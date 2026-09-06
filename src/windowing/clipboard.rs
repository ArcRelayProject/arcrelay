use super::*;

pub fn ensure_clipboard_window(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "ensure clipboard window", |app| {
            ensure_clipboard_window_on_main(&app)
        })
    }
    #[cfg(not(target_os = "macos"))]
    ensure_clipboard_window_on_main(app)
}

fn ensure_clipboard_window_on_main(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    if app.get_webview_window(CLIPBOARD_WINDOW_LABEL).is_some() {
        return Ok(());
    }

    if CREATING_CLIPBOARD_WINDOW.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let saved_size = load_clipboard_window_size();
    let width = saved_size
        .as_ref()
        .map_or(CLIPBOARD_WINDOW_WIDTH, |size| size.width)
        .max(CLIPBOARD_WINDOW_MIN_WIDTH);
    let height = saved_size
        .as_ref()
        .map_or(CLIPBOARD_WINDOW_HEIGHT, |size| size.height)
        .max(CLIPBOARD_WINDOW_MIN_HEIGHT);
    let result = WebviewWindowBuilder::new(
        app,
        CLIPBOARD_WINDOW_LABEL,
        WebviewUrl::App("clipboard.html".into()),
    )
    .title("ArcRelay 剪贴板")
    .inner_size(width, height)
    .min_inner_size(CLIPBOARD_WINDOW_MIN_WIDTH, CLIPBOARD_WINDOW_MIN_HEIGHT)
    .resizable(true)
    .decorations(false)
    .transparent(true)
    .shadow(true)
    .always_on_top(false)
    .skip_taskbar(true)
    .accept_first_mouse(true)
    .visible(false)
    .build()
    .map(|window| {
        if let Err(error) = configure_platform_clipboard_window(&window) {
            tracing::warn!(%error, "Failed to configure clipboard window");
        }
        let _ = window.hide();
    });

    CREATING_CLIPBOARD_WINDOW.store(false, Ordering::SeqCst);
    result
}

pub fn show_clipboard_window(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "show clipboard window", |app| {
            show_clipboard_window_on_main(&app)
        })
    }
    #[cfg(not(target_os = "macos"))]
    show_clipboard_window_on_main(app)
}

fn show_clipboard_window_on_main(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    if app
        .try_state::<crate::backend::DesktopState>()
        .is_some_and(|state| !state.settings.snapshot().clipboard_enabled)
    {
        return Ok(());
    }
    ensure_clipboard_window_on_main(app)?;
    let Some(window) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) else {
        return Ok(());
    };

    show_platform_clipboard_window(app, &window)?;
    // Notify the webview only after the native window can receive keyboard
    // input, so focusing the search field is not lost while the panel is hidden.
    let _ = window.emit("clipboard-window-shown", ());
    Ok(())
}

pub fn hide_clipboard_window(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "hide clipboard window", |app| {
            hide_clipboard_window_on_main(&app)
        })
    }
    #[cfg(not(target_os = "macos"))]
    hide_clipboard_window_on_main(app)
}

fn hide_clipboard_window_on_main(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    let Some(window) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) else {
        return Ok(());
    };
    #[cfg(target_os = "macos")]
    stop_clipboard_outside_click_monitor();
    let _ = window.emit("clipboard-window-hidden", ());
    crate::commands::clear_clipboard_thumbnail_cache();
    if let Some(state) = app.try_state::<crate::backend::DesktopState>() {
        state.text_selection.clear();
    }
    hide_platform_clipboard_window(app, &window)
}

pub fn clipboard_window_pinned() -> bool {
    CLIPBOARD_WINDOW_PINNED.load(Ordering::SeqCst)
}

/// Relinquish keyboard focus before emitting a synthetic paste without
/// changing the visible state of a pinned clipboard window.
pub fn prepare_clipboard_window_for_paste(app: &AppHandle) -> tauri::Result<()> {
    if !clipboard_window_pinned() {
        return hide_clipboard_window(app);
    }

    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "prepare pinned clipboard paste", |app| {
            if let Ok(panel) = app.get_webview_panel(CLIPBOARD_WINDOW_LABEL) {
                panel.resign_key_window();
            }
            Ok(())
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(target_os = "windows")]
        restore_previous_foreground_window();
        let _ = app;
        Ok(())
    }
}

pub fn clipboard_context_menu_open() -> bool {
    CLIPBOARD_CONTEXT_MENU_OPEN.load(Ordering::SeqCst)
}

pub fn set_clipboard_context_menu_open(open: bool) {
    CLIPBOARD_CONTEXT_MENU_OPEN.store(open, Ordering::SeqCst);
}

pub fn set_clipboard_window_pinned(app: &AppHandle, pinned: bool) -> tauri::Result<()> {
    CLIPBOARD_WINDOW_PINNED.store(pinned, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "pin clipboard window", move |app| {
            if let Some(window) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
                set_platform_clipboard_window_pinned(&app, &window, pinned)?;
                let _ = window.emit("clipboard-window-pin-changed", pinned);
            }
            Ok(())
        })
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(window) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        set_platform_clipboard_window_pinned(app, &window, pinned)?;
        let _ = window.emit("clipboard-window-pin-changed", pinned);
    }
    #[cfg(not(target_os = "macos"))]
    Ok(())
}

pub fn start_clipboard_window_drag(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "drag clipboard window", |app| {
            if let Some(window) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
                window.start_dragging()?;
            }
            Ok(())
        })
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(window) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
        window.start_dragging()?;
    }
    #[cfg(not(target_os = "macos"))]
    Ok(())
}

pub fn schedule_clipboard_window_size_save(app: &AppHandle) {
    let revision = CLIPBOARD_SIZE_SAVE_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(220)).await;
        if CLIPBOARD_SIZE_SAVE_REVISION.load(Ordering::SeqCst) != revision {
            return;
        }
        #[cfg(target_os = "macos")]
        if let Err(error) = dispatch_appkit(&app, "read clipboard window size", move |app| {
            let Ok(size) = capture_clipboard_window_size(&app) else {
                return Ok(());
            };
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = save_clipboard_window_size_value(&size) {
                    tracing::warn!(%error, "Failed to save clipboard window size");
                }
            });
            Ok(())
        }) {
            tracing::warn!(%error, "Failed to schedule clipboard size read");
        }
        #[cfg(not(target_os = "macos"))]
        if let Ok(size) = capture_clipboard_window_size(&app) {
            if let Err(error) = save_clipboard_window_size_value(&size) {
                tracing::warn!(%error, "Failed to save clipboard window size");
            }
        }
    });
}

pub fn restore_clipboard_window_size_after_scale_change(app: &AppHandle) {
    let Some(size) = load_clipboard_window_size() else {
        return;
    };
    if app.get_webview_window(CLIPBOARD_WINDOW_LABEL).is_none() {
        return;
    }
    CLIPBOARD_SIZE_SAVE_REVISION.fetch_add(1, Ordering::SeqCst);
    let restore_revision = CLIPBOARD_SCALE_RESTORE_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        if CLIPBOARD_SCALE_RESTORE_REVISION.load(Ordering::SeqCst) != restore_revision {
            return;
        }
        #[cfg(target_os = "macos")]
        let _ = dispatch_appkit(&app, "restore clipboard window size", move |app| {
            if let Some(window) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
                let _ = window.set_size(Size::Logical(LogicalSize::new(size.width, size.height)));
            }
            Ok(())
        });
        #[cfg(not(target_os = "macos"))]
        if let Some(window) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) {
            let _ = window.set_size(Size::Logical(LogicalSize::new(size.width, size.height)));
        }
    });
}

fn clipboard_window_size_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ArcRelay")
        .join("clipboard-window.json")
}

fn load_clipboard_window_size() -> Option<ClipboardWindowSize> {
    let bytes = std::fs::read(clipboard_window_size_path()).ok()?;
    let size: ClipboardWindowSize = serde_json::from_slice(&bytes).ok()?;
    (size.width.is_finite()
        && size.height.is_finite()
        && size.width >= CLIPBOARD_WINDOW_MIN_WIDTH
        && size.height >= CLIPBOARD_WINDOW_MIN_HEIGHT)
        .then_some(size)
}

fn capture_clipboard_window_size(app: &AppHandle) -> Result<ClipboardWindowSize, String> {
    let window = app
        .get_webview_window(CLIPBOARD_WINDOW_LABEL)
        .ok_or_else(|| "clipboard window is unavailable".to_string())?;
    let physical = window.inner_size().map_err(|error| error.to_string())?;
    let scale = window.scale_factor().map_err(|error| error.to_string())?;
    Ok(ClipboardWindowSize {
        width: f64::from(physical.width) / scale,
        height: f64::from(physical.height) / scale,
    })
}

fn save_clipboard_window_size_value(size: &ClipboardWindowSize) -> Result<(), String> {
    let path = clipboard_window_size_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec(&size).map_err(|error| error.to_string())?;
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

pub fn toggle_clipboard_window(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "toggle clipboard window", |app| {
            let visible = app
                .get_webview_window(CLIPBOARD_WINDOW_LABEL)
                .and_then(|window| window.is_visible().ok())
                .unwrap_or(false);
            let result = if visible {
                hide_clipboard_window_on_main(&app)
            } else {
                show_clipboard_window_on_main(&app)
            };
            if let Err(error) = result {
                tracing::warn!(%error, "Failed to toggle clipboard window");
            }
            Ok(())
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let visible = app
            .get_webview_window(CLIPBOARD_WINDOW_LABEL)
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
        if visible {
            hide_clipboard_window(app)
        } else {
            show_clipboard_window(app)
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn position_clipboard_window(app: &AppHandle, window: &tauri::WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        position_clipboard_window_windows(app, window);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        position_clipboard_window_generic(app, window);
    }
}

#[cfg(target_os = "macos")]
fn configure_platform_clipboard_window(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let panel = window.to_panel::<ClipboardPanel>()?;
    panel.set_level(CLIPBOARD_PANEL_LEVEL);
    panel.set_floating_panel(false);
    panel.set_has_shadow(true);
    panel.set_opaque(false);
    panel.set_transparent(true);
    panel.set_corner_radius(CLIPBOARD_WINDOW_CORNER_RADIUS);
    let content = panel.content_view();
    content.setWantsLayer(true);
    if let Some(layer) = content.layer() {
        layer.setCornerRadius(CLIPBOARD_WINDOW_CORNER_RADIUS);
        layer.setMasksToBounds(true);
    }
    panel.set_hides_on_deactivate(false);
    panel.set_released_when_closed(false);
    // The non-activating panel must still become the key window when opened so
    // its WKWebView can receive keydown events without activating ArcRelay.
    panel.set_becomes_key_only_if_needed(false);
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().resizable().value());
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .value(),
    );
    panel.hide();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn configure_platform_clipboard_window(_window: &tauri::WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn show_platform_clipboard_window(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
) -> tauri::Result<()> {
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    position_clipboard_window_macos(window);
    if let Ok(panel) = app.get_webview_panel(CLIPBOARD_WINDOW_LABEL) {
        // A clipboard chooser must not activate ArcRelay or steal the
        // insertion target from the application that invoked it.
        panel.show_and_make_key();
        start_clipboard_outside_click_monitor(app.clone());
        return Ok(());
    }
    window.show()?;
    window.set_focus()?;
    start_clipboard_outside_click_monitor(app.clone());
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn show_platform_clipboard_window(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
) -> tauri::Result<()> {
    #[cfg(target_os = "windows")]
    remember_foreground_window();
    position_clipboard_window(app, window);
    window.show()?;
    window.set_focus()
}

#[cfg(target_os = "windows")]
fn remember_foreground_window() {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let foreground = unsafe { GetForegroundWindow() };
    if !foreground.0.is_null() {
        CLIPBOARD_PREVIOUS_FOREGROUND_WINDOW.store(foreground.0 as isize, Ordering::SeqCst);
    }
}

#[cfg(target_os = "windows")]
fn restore_previous_foreground_window() {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

    let raw = CLIPBOARD_PREVIOUS_FOREGROUND_WINDOW.load(Ordering::SeqCst);
    if raw != 0 {
        let _ = unsafe { SetForegroundWindow(HWND(raw as *mut std::ffi::c_void)) };
    }
}

#[cfg(target_os = "macos")]
fn hide_platform_clipboard_window(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
) -> tauri::Result<()> {
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    if let Ok(panel) = app.get_webview_panel(CLIPBOARD_WINDOW_LABEL) {
        // If the user clicked into the panel (for example, the search
        // field), hand key status back before synthesizing Cmd+V.
        panel.resign_key_window();
        panel.hide();
        return Ok(());
    }
    window.hide()
}

#[cfg(not(target_os = "macos"))]
fn hide_platform_clipboard_window(
    _app: &AppHandle,
    window: &tauri::WebviewWindow,
) -> tauri::Result<()> {
    window.hide()
}

#[cfg(target_os = "macos")]
fn set_platform_clipboard_window_pinned(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    pinned: bool,
) -> tauri::Result<()> {
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    if let Ok(panel) = app.get_webview_panel(CLIPBOARD_WINDOW_LABEL) {
        panel.set_floating_panel(pinned);
        panel.set_level(CLIPBOARD_PANEL_LEVEL);
        return Ok(());
    }
    window.set_always_on_top(pinned)
}

#[cfg(not(target_os = "macos"))]
fn set_platform_clipboard_window_pinned(
    _app: &AppHandle,
    window: &tauri::WebviewWindow,
    pinned: bool,
) -> tauri::Result<()> {
    window.set_always_on_top(pinned)
}

#[cfg(target_os = "macos")]
fn position_clipboard_window_macos(window: &tauri::WebviewWindow) {
    let cursor = objc2_app_kit::NSEvent::mouseLocation();
    let Ok(monitors) = window.available_monitors() else {
        let _ = window.center();
        return;
    };
    let main_height = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| f64::from(monitor.size().height) / monitor.scale_factor())
        .unwrap_or(0.0);
    let cursor_logical = LogicalPosition::new(cursor.x, main_height - cursor.y);
    let Some(monitor) = monitors.into_iter().find(|monitor| {
        let scale = monitor.scale_factor();
        let position = monitor.position().to_logical::<f64>(scale);
        let size = monitor.size().to_logical::<f64>(scale);
        cursor_logical.x >= position.x
            && cursor_logical.y >= position.y
            && cursor_logical.x < position.x + size.width
            && cursor_logical.y < position.y + size.height
    }) else {
        let _ = window.center();
        return;
    };
    let Ok(window_size) = window.outer_size() else {
        let _ = window.center();
        return;
    };
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let cursor_physical = cursor_logical.to_physical::<f64>(scale);
    let area_position = work_area.position;
    let area_size = work_area.size;
    let x = clamp_window_axis(
        cursor_physical.x + CLIPBOARD_WINDOW_CURSOR_OFFSET * scale,
        f64::from(area_position.x),
        f64::from(area_size.width),
        f64::from(window_size.width),
    );
    let y = clamp_window_axis(
        cursor_physical.y + CLIPBOARD_WINDOW_CURSOR_OFFSET * scale,
        f64::from(area_position.y),
        f64::from(area_size.height),
        f64::from(window_size.height),
    );
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(
        x.round() as i32,
        y.round() as i32,
    )));
}

#[cfg(target_os = "macos")]
fn start_clipboard_outside_click_monitor(app: AppHandle) {
    let revision = CLIPBOARD_OUTSIDE_CLICK_MONITOR_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    let initially_down = objc2_app_kit::NSEvent::pressedMouseButtons() & 1 != 0;
    let _ = std::thread::Builder::new()
        .name("arcrelay-clipboard-click".into())
        .spawn(move || {
            let mut was_down = initially_down;
            while CLIPBOARD_OUTSIDE_CLICK_MONITOR_REVISION.load(Ordering::SeqCst) == revision {
                std::thread::sleep(CLIPBOARD_OUTSIDE_CLICK_POLL_INTERVAL);
                let mouse_down = macos_left_mouse_button_down();
                let pressed = mouse_down && !was_down;
                was_down = mouse_down;
                if !pressed || clipboard_window_pinned() || clipboard_context_menu_open() {
                    continue;
                }

                let should_stop =
                    dispatch_appkit(&app, "handle clipboard outside click", move |app| {
                        if CLIPBOARD_OUTSIDE_CLICK_MONITOR_REVISION.load(Ordering::SeqCst)
                            != revision
                            || clipboard_window_pinned()
                            || clipboard_context_menu_open()
                        {
                            return Ok(true);
                        }
                        let Some(window) = app.get_webview_window(CLIPBOARD_WINDOW_LABEL) else {
                            stop_clipboard_outside_click_monitor();
                            return Ok(true);
                        };
                        if !window.is_visible().unwrap_or(false) {
                            stop_clipboard_outside_click_monitor();
                            return Ok(true);
                        }
                        if !cursor_is_inside_window(&app, &window) {
                            if let Err(error) = hide_clipboard_window_on_main(&app) {
                                tracing::warn!(
                                    %error,
                                    "failed to hide clipboard window after outside click"
                                );
                            }
                            return Ok(true);
                        }
                        Ok(false)
                    });
                match should_stop {
                    Ok(true) | Err(_) => break,
                    Ok(false) => continue,
                }
            }
        });
}

#[cfg(target_os = "macos")]
fn macos_left_mouse_button_down() -> bool {
    type CGEventSourceStateID = i32;
    type CGMouseButton = u32;
    const COMBINED_SESSION_STATE: CGEventSourceStateID = 0;
    const LEFT_MOUSE_BUTTON: CGMouseButton = 0;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceButtonState(state_id: CGEventSourceStateID, button: CGMouseButton) -> bool;
    }
    unsafe { CGEventSourceButtonState(COMBINED_SESSION_STATE, LEFT_MOUSE_BUTTON) }
}

#[cfg(target_os = "macos")]
fn stop_clipboard_outside_click_monitor() {
    CLIPBOARD_OUTSIDE_CLICK_MONITOR_REVISION.fetch_add(1, Ordering::SeqCst);
}

#[cfg(target_os = "macos")]
fn cursor_is_inside_window(app: &AppHandle, window: &tauri::WebviewWindow) -> bool {
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    let Ok(cursor) = app.cursor_position() else {
        // Be conservative when macOS cannot provide a cursor position: an
        // accidental hide is more disruptive than waiting for the next click.
        return true;
    };
    let Ok(position) = window.outer_position() else {
        return true;
    };
    let Ok(size) = window.outer_size() else {
        return true;
    };

    let left = f64::from(position.x);
    let top = f64::from(position.y);
    cursor.x >= left
        && cursor.x < left + f64::from(size.width)
        && cursor.y >= top
        && cursor.y < top + f64::from(size.height)
}

#[cfg(target_os = "windows")]
fn position_clipboard_window_windows(app: &AppHandle, window: &tauri::WebviewWindow) {
    position_clipboard_window_physical(app, window, true);
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn position_clipboard_window_generic(app: &AppHandle, window: &tauri::WebviewWindow) {
    position_clipboard_window_physical(app, window, false);
}

#[cfg(not(target_os = "macos"))]
fn position_clipboard_window_physical(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    use_work_area: bool,
) {
    let Ok(cursor) = app.cursor_position() else {
        let _ = window.center();
        return;
    };
    let Ok(monitors) = window.available_monitors() else {
        let _ = window.center();
        return;
    };
    let Some(monitor) = monitors.into_iter().find(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        cursor.x >= f64::from(position.x)
            && cursor.y >= f64::from(position.y)
            && cursor.x < f64::from(position.x) + f64::from(size.width)
            && cursor.y < f64::from(position.y) + f64::from(size.height)
    }) else {
        let _ = window.center();
        return;
    };
    let Ok(window_size) = window.outer_size() else {
        let _ = window.center();
        return;
    };
    let (area_position, area_size) = if use_work_area {
        let work_area = monitor.work_area();
        (work_area.position, work_area.size)
    } else {
        (*monitor.position(), *monitor.size())
    };
    let x = clamp_window_axis(
        cursor.x + CLIPBOARD_WINDOW_CURSOR_OFFSET,
        f64::from(area_position.x),
        f64::from(area_size.width),
        f64::from(window_size.width),
    )
    .round() as i32;
    let y = clamp_window_axis(
        cursor.y + CLIPBOARD_WINDOW_CURSOR_OFFSET,
        f64::from(area_position.y),
        f64::from(area_size.height),
        f64::from(window_size.height),
    )
    .round() as i32;
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
}

fn clamp_window_axis(desired: f64, origin: f64, span: f64, window_span: f64) -> f64 {
    let minimum = origin + CLIPBOARD_WINDOW_EDGE_MARGIN;
    let maximum = (origin + span - window_span - CLIPBOARD_WINDOW_EDGE_MARGIN).max(minimum);
    desired.clamp(minimum, maximum)
}
