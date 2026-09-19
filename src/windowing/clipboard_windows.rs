//! Windows nonactivating clipboard surface. Hooks live on one message thread;
//! callbacks never perform IPC, paste, or wait for the UI thread. Hidden/idle
//! sessions pass through, except releases paired with already consumed presses.

use super::clipboard_windows_policy::{
    navigation_key, ClipboardNavigationKey, ClipboardSession, ForegroundTarget,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::{mpsc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

static SESSION: OnceLock<Mutex<ClipboardSession>> = OnceLock::new();
static HOOKS: Mutex<Option<u32>> = Mutex::new(None);
static WINDOW: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static UI_THREAD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
const SUBCLASS_ID: usize = 0x41524343;

pub fn initialize() {
    UI_THREAD.store(
        unsafe { windows::Win32::System::Threading::GetCurrentThreadId() },
        std::sync::atomic::Ordering::Release,
    );
}

pub fn dispatch_ui<T: Send + 'static>(
    app: &AppHandle,
    task: impl FnOnce(AppHandle) -> tauri::Result<T> + Send + 'static,
) -> tauri::Result<T> {
    if UI_THREAD.load(std::sync::atomic::Ordering::Acquire)
        == unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }
    {
        return task(app.clone());
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let handle = app.clone();
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let check = cancelled.clone();
    app.run_on_main_thread(move || {
        if !check.load(std::sync::atomic::Ordering::Acquire) {
            let _ = tx.send(task(handle));
        }
    })?;
    rx.recv_timeout(std::time::Duration::from_secs(3))
        .map_err(|error| {
            cancelled.store(true, std::sync::atomic::Ordering::Release);
            tauri::Error::Io(std::io::Error::other(error.to_string()))
        })?
}

unsafe extern "system" fn window_subclass(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if message == WM_MOUSEACTIVATE
        && GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_NOACTIVATE.0 as isize != 0
    {
        // Includes accessibility hover activation; don't eat the mouse click.
        return LRESULT(MA_NOACTIVATE as isize);
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(window_subclass), SUBCLASS_ID);
    }
    DefSubclassProc(window, message, wparam, lparam)
}

fn session() -> &'static Mutex<ClipboardSession> {
    SESSION.get_or_init(|| Mutex::new(ClipboardSession::default()))
}

enum HookEvent {
    Key(ClipboardNavigationKey),
    Pause(u64),
}

thread_local! {
    static EVENTS: RefCell<Option<mpsc::SyncSender<HookEvent>>> = const { RefCell::new(None) };
    static CONSUMED: RefCell<HashSet<u32>> = RefCell::new(HashSet::new());
}

fn send(event: HookEvent) -> bool {
    EVENTS.with(|sender| {
        sender
            .borrow()
            .as_ref()
            .is_some_and(|sender| sender.try_send(event).is_ok())
    })
}

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut std::ffi::c_void)
}

fn native_focusable(window: HWND, focusable: bool) -> Result<(), String> {
    unsafe {
        let old = GetWindowLongPtrW(window, GWL_EXSTYLE);
        let flag = WS_EX_NOACTIVATE.0 as isize;
        let new = if focusable { old & !flag } else { old | flag };
        if old != new {
            windows::Win32::Foundation::SetLastError(windows::Win32::Foundation::ERROR_SUCCESS);
            let previous = SetWindowLongPtrW(window, GWL_EXSTYLE, new);
            let error = windows::Win32::Foundation::GetLastError();
            if previous == 0 && error != windows::Win32::Foundation::ERROR_SUCCESS {
                return Err(windows::core::Error::from_win32().to_string());
            }
        }
    }
    Ok(())
}

pub fn foreground_target() -> Option<ForegroundTarget> {
    unsafe {
        let window = GetForegroundWindow();
        if window.0.is_null() {
            return None;
        }
        let mut process = 0;
        let thread = GetWindowThreadProcessId(window, Some(&mut process));
        if process == 0 || process == std::process::id() {
            return None;
        }
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(thread, &mut info).is_err() {
            return None;
        }
        Some(ForegroundTarget {
            window: window.0 as isize,
            process,
            thread,
            focus: info.hwndFocus.0 as isize,
        })
    }
}

fn valid(target: ForegroundTarget) -> bool {
    let mut process = 0;
    unsafe {
        IsWindow(Some(hwnd(target.window))).as_bool()
            && GetWindowThreadProcessId(hwnd(target.window), Some(&mut process)) == target.thread
            && process == target.process
    }
}

pub fn capture_paste_target() -> Result<ForegroundTarget, String> {
    if let Some(target) = foreground_target() {
        return Ok(target);
    }
    let state = session().lock().unwrap_or_else(|error| error.into_inner());
    state
        .target
        .filter(|target| valid(*target))
        .ok_or_else(|| "no external paste target; content remains on clipboard".into())
}

pub fn target_ready(target: ForegroundTarget) -> Result<bool, String> {
    if !valid(target) {
        return Err("paste target closed; content remains on clipboard".into());
    }
    match foreground_target() {
        Some(current) if current == target => Ok(true),
        Some(_) => Err("paste target changed; content remains on clipboard".into()),
        None => Ok(false),
    }
}

pub fn modifiers_released() -> bool {
    [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
        .into_iter()
        .all(|key| unsafe { GetAsyncKeyState(key.0 as i32) } as u16 & 0x8000 == 0)
}

pub fn configure(window: &WebviewWindow) -> tauri::Result<()> {
    WINDOW.store(
        window.hwnd()?.0 as isize,
        std::sync::atomic::Ordering::Release,
    );
    if !unsafe {
        SetWindowSubclass(window.hwnd()?, Some(window_subclass), SUBCLASS_ID, 0).as_bool()
    } {
        return Err(tauri::Error::Io(std::io::Error::other(
            "clipboard mouse activation guard could not be installed",
        )));
    }
    native_focusable(window.hwnd()?, false)
        .map_err(|error| tauri::Error::Io(std::io::Error::other(error)))
}

pub fn raise(window: &WebviewWindow) -> tauri::Result<()> {
    unsafe {
        SetWindowPos(
            window.hwnd()?,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .map_err(|error| tauri::Error::Io(std::io::Error::other(error.to_string())))?;
    }
    Ok(())
}

fn show_native(window: HWND) -> Result<(), String> {
    unsafe {
        SetWindowPos(
            window,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .map_err(|error| error.to_string())?;
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
    }
    Ok(())
}

pub fn show(app: &AppHandle, window: &WebviewWindow) -> tauri::Result<()> {
    start_hooks(app).map_err(|error| tauri::Error::Io(std::io::Error::other(error)))?;
    configure(window)?;
    session()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .show(foreground_target());
    let native = window.hwnd()?;
    show_native(native).map_err(|error| tauri::Error::Io(std::io::Error::other(error)))?;
    Ok(())
}

pub fn hidden(window: &WebviewWindow) -> tauri::Result<()> {
    session()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .hide();
    native_focusable(window.hwnd()?, false)
        .map_err(|error| tauri::Error::Io(std::io::Error::other(error)))?;
    // Native show deliberately bypasses Tao's activating show path, so hide
    // must not rely on Tao's cached VISIBLE flag either.
    let _ = unsafe { ShowWindow(window.hwnd()?, SW_HIDE) };
    Ok(())
}

pub fn destroyed() {
    WINDOW.store(0, std::sync::atomic::Ordering::Release);
    let mut state = session().lock().unwrap_or_else(|error| error.into_inner());
    state.hide();
    state.ready = false;
}

pub fn navigation_ready(ready: bool) -> u64 {
    let mut state = session().lock().unwrap_or_else(|error| error.into_inner());
    state.ready = ready;
    state.generation
}

pub fn activate() -> u64 {
    let mut state = session().lock().unwrap_or_else(|error| error.into_inner());
    if state.visible && !state.editing {
        if let Some(target) = foreground_target() {
            if !state.selecting || state.target != Some(target) {
                state.generation += 1;
                state.target = Some(target);
                state.selecting = true;
            }
        }
    }
    state.generation
}

pub fn set_editing(app: &AppHandle, editing: bool) -> Result<u64, String> {
    let window = app
        .get_webview_window(super::CLIPBOARD_WINDOW_LABEL)
        .ok_or("clipboard window is unavailable")?;
    let native = window.hwnd().map_err(|error| error.to_string())?;
    let (previous, generation) = {
        let mut state = session().lock().unwrap_or_else(|error| error.into_inner());
        if !state.visible {
            return Ok(state.generation);
        }
        if state.editing == editing {
            return Ok(state.generation);
        }
        if editing {
            // Save before activating; retain a valid existing recipient only
            // when the clipboard already owns foreground focus.
            if let Some(target) = foreground_target() {
                state.target = Some(target);
            }
        }
        state.generation += 1;
        state.editing = editing;
        state.selecting = !editing;
        (state.target, state.generation)
    };
    // Tao applies show/hide as a side effect of changing window flags. Toggle
    // only the native noactivate style so exiting editing after Alt+Tab cannot
    // incidentally reactivate the clipboard before we inspect foreground.
    native_focusable(native, editing)?;
    if editing {
        if unsafe { GetForegroundWindow() != native && !SetForegroundWindow(native).as_bool() } {
            let _ = native_focusable(native, false);
            let mut state = session().lock().unwrap_or_else(|error| error.into_inner());
            state.editing = false;
            state.pause();
            return Err("Windows rejected activation for clipboard editing; click the search field to retry".into());
        }
        // Focus the WebView itself, not Tao's cached (natively shown) window.
        let webview: &tauri::Webview = window.as_ref();
        webview.set_focus().map_err(|error| error.to_string())?;
    } else if unsafe { GetForegroundWindow() == native } {
        if let Some(previous) = previous.filter(|target| valid(*target)) {
            if !unsafe { SetForegroundWindow(hwnd(previous.window)).as_bool() } {
                return Err("Windows rejected restoration of the paste target".into());
            }
        }
    }
    Ok(generation)
}

pub fn prepare_paste(app: &AppHandle) -> Result<(), String> {
    set_editing(app, false)?;
    session()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .pause();
    let _ = app.emit_to(
        super::CLIPBOARD_WINDOW_LABEL,
        "clipboard-navigation-paused",
        (),
    );
    Ok(())
}

pub async fn on_main<T: Send + 'static>(
    app: &AppHandle,
    task: impl FnOnce(AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if !tx.is_closed() {
            let _ = tx.send(task(handle));
        }
    })
    .map_err(|error| error.to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(3), rx)
        .await
        .map_err(|_| "clipboard UI operation timed out".to_string())?
        .map_err(|error| error.to_string())?
}

pub fn shutdown() {
    if let Some(thread) = HOOKS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        let _ = unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
    }
}

fn pause_from_hook() {
    if crate::clipboard_drag::is_dragging() {
        return;
    }
    let generation = if let Ok(mut state) = session().try_lock() {
        if !state.visible || (!state.selecting && !state.editing) {
            return;
        }
        state.pause();
        state.generation
    } else {
        return;
    };
    send(HookEvent::Pause(generation));
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let event = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    if event.flags.contains(LLKHF_INJECTED) {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let message = wparam.0 as u32;
    if matches!(message, WM_KEYUP | WM_SYSKEYUP) {
        if CONSUMED.with(|keys| keys.borrow_mut().remove(&event.vkCode)) {
            return LRESULT(1);
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }
    if crate::clipboard_drag::is_dragging() {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    if !matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let repeated = CONSUMED.with(|keys| keys.borrow().contains(&event.vkCode));
    let Some(target) = foreground_target() else {
        return if repeated {
            LRESULT(1)
        } else {
            CallNextHookEx(None, code, wparam, lparam)
        };
    };
    let generation = match session().try_lock() {
        Ok(state) if state.captures(Some(target)) => state.generation,
        _ => {
            return if repeated {
                LRESULT(1)
            } else {
                CallNextHookEx(None, code, wparam, lparam)
            }
        }
    };
    let pressed = |key: i32| GetAsyncKeyState(key) as u16 & 0x8000 != 0;
    let ctrl = pressed(VK_CONTROL.0 as i32);
    let shift = pressed(VK_SHIFT.0 as i32);
    let alt = pressed(VK_MENU.0 as i32);
    let win = pressed(VK_LWIN.0 as i32) || pressed(VK_RWIN.0 as i32);
    if let Some((key, code)) = navigation_key(event.vkCode, ctrl, shift, alt, win) {
        if send(HookEvent::Key(ClipboardNavigationKey {
            key: key.into(),
            code,
            ctrl_key: ctrl,
            shift_key: shift,
            alt_key: alt,
            meta_key: win,
            repeat: repeated,
            generation,
        })) || repeated
        {
            CONSUMED.with(|keys| keys.borrow_mut().insert(event.vkCode));
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && matches!(
            wparam.0 as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN
        )
    {
        let native = hwnd(WINDOW.load(std::sync::atomic::Ordering::Acquire));
        let mut rect = windows::Win32::Foundation::RECT::default();
        if !native.0.is_null() && GetWindowRect(native, &mut rect).is_ok() {
            let point = (*(lparam.0 as *const MSLLHOOKSTRUCT)).pt;
            if point.x < rect.left
                || point.x >= rect.right
                || point.y < rect.top
                || point.y >= rect.bottom
            {
                pause_from_hook();
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn foreground_hook(
    _: HWINEVENTHOOK,
    _: u32,
    window: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    let mut process = 0;
    GetWindowThreadProcessId(window, Some(&mut process));
    if process == std::process::id()
        && window.0 as isize == WINDOW.load(std::sync::atomic::Ordering::Acquire)
    {
        return;
    }
    let changed = session().try_lock().is_ok_and(|state| {
        state.visible
            && (state.editing
                || state
                    .target
                    .is_some_and(|target| target.window != window.0 as isize))
    });
    if changed {
        pause_from_hook();
    }
}

fn start_hooks(app: &AppHandle) -> Result<(), String> {
    let mut active = HOOKS.lock().unwrap_or_else(|error| error.into_inner());
    if active.is_some() {
        return Ok(());
    }
    let (events_tx, events_rx) = mpsc::sync_channel(64);
    let (started_tx, started_rx) = mpsc::sync_channel::<Result<u32, String>>(1);
    let app = app.clone();
    std::thread::Builder::new()
        .name("clipboard-window-events".into())
        .spawn(move || {
            while let Ok(event) = events_rx.recv() {
                let app = app.clone();
                let _ = app.clone().run_on_main_thread(move || match event {
                    HookEvent::Key(key) => {
                        let current = session().lock().unwrap_or_else(|error| error.into_inner());
                        if current.generation == key.generation
                            && current.captures(foreground_target())
                        {
                            let _ = app.emit_to(
                                super::CLIPBOARD_WINDOW_LABEL,
                                "clipboard-navigation-key",
                                key,
                            );
                        }
                    }
                    HookEvent::Pause(generation) => {
                        if crate::clipboard_drag::is_dragging() {
                            return;
                        }
                        if session()
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .generation
                            != generation
                        {
                            return;
                        }
                        let _ = set_editing(&app, false);
                        session()
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .pause();
                        let _ = app.emit_to(
                            super::CLIPBOARD_WINDOW_LABEL,
                            "clipboard-navigation-paused",
                            (),
                        );
                        if !super::clipboard_window_pinned() {
                            let _ = super::hide_clipboard_window(&app);
                        }
                    }
                });
            }
        })
        .map_err(|error| error.to_string())?;
    std::thread::Builder::new()
        .name("clipboard-window-hooks".into())
        .spawn(move || unsafe {
            EVENTS.with(|sender| *sender.borrow_mut() = Some(events_tx));
            // Prime the message queue before publishing the thread ID.
            let mut message = MSG::default();
            let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0);
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0);
            let foreground = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_hook),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            match (keyboard, mouse) {
                (Ok(keyboard), Ok(mouse)) if !foreground.0.is_null() => {
                    let _ = started_tx
                        .send(Ok(windows::Win32::System::Threading::GetCurrentThreadId()));
                    while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    let _ = UnhookWindowsHookEx(keyboard);
                    let _ = UnhookWindowsHookEx(mouse);
                }
                (keyboard, mouse) => {
                    if let Ok(keyboard) = keyboard {
                        let _ = UnhookWindowsHookEx(keyboard);
                    }
                    if let Ok(mouse) = mouse {
                        let _ = UnhookWindowsHookEx(mouse);
                    }
                    let _ = started_tx.send(Err(
                        "Windows clipboard input hooks could not be installed".into(),
                    ));
                }
            }
            if !foreground.0.is_null() {
                let _ = UnhookWinEvent(foreground);
            }
            EVENTS.with(|sender| sender.borrow_mut().take());
        })
        .map_err(|error| error.to_string())?;
    *active = Some(
        started_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|error| error.to_string())??,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_show_reopen_and_style_changes_preserve_foreground_without_tokio() {
        unsafe {
            let window = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                windows::core::w!("STATIC"),
                windows::core::w!("ArcRelay nonactivating focus test"),
                WS_POPUP,
                0,
                0,
                32,
                32,
                None,
                None,
                None,
                None,
            )
            .expect("create native focus test window");
            struct Cleanup(HWND);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    let _ = unsafe { DestroyWindow(self.0) };
                }
            }
            let _cleanup = Cleanup(window);
            assert!(SetWindowSubclass(window, Some(window_subclass), SUBCLASS_ID, 0).as_bool());
            let foreground = GetForegroundWindow();
            for _ in 0..2 {
                native_focusable(window, false).unwrap();
                show_native(window).unwrap();
                assert_eq!(GetForegroundWindow(), foreground);
                assert!(IsWindowVisible(window).as_bool());
                assert_eq!(
                    SendMessageW(window, WM_MOUSEACTIVATE, Some(WPARAM(0)), Some(LPARAM(0))),
                    LRESULT(MA_NOACTIVATE as isize)
                );
                native_focusable(window, true).unwrap();
                assert_eq!(
                    GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_NOACTIVATE.0 as isize,
                    0
                );
                native_focusable(window, false).unwrap();
                assert_eq!(GetForegroundWindow(), foreground);
                let _ = ShowWindow(window, SW_HIDE);
                assert!(!IsWindowVisible(window).as_bool());
            }
        }
    }
}
