use super::*;

pub fn ensure_permission_guide_window(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "ensure permission guide window", |app| {
            ensure_permission_guide_window_on_main(&app)
        })
    }
    #[cfg(not(target_os = "macos"))]
    ensure_permission_guide_window_on_main(app)
}

pub fn close_permission_guide_window(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "close permission guide window", |app| {
            close_permission_guide_window_on_main(&app)
        })
    }
    #[cfg(not(target_os = "macos"))]
    close_permission_guide_window_on_main(app)
}

fn close_permission_guide_window_on_main(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    PERMISSION_GUIDE_WINDOW_READY.store(false, Ordering::SeqCst);
    PERMISSION_GUIDE_WINDOW_CREATED_AT_MS.store(0, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(PERMISSION_GUIDE_WINDOW_LABEL) {
        window.destroy()?;
    }
    Ok(())
}

pub fn permission_guide_window_ready(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "show ready permission guide window", |app| {
            show_permission_guide_window_on_main(&app)
        })
    }
    #[cfg(not(target_os = "macos"))]
    show_permission_guide_window_on_main(app)
}

fn show_permission_guide_window_on_main(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    let Some(window) = app.get_webview_window(PERMISSION_GUIDE_WINDOW_LABEL) else {
        PERMISSION_GUIDE_WINDOW_READY.store(false, Ordering::SeqCst);
        return Ok(());
    };
    position_permission_guide_at_screen_edge(&window);
    let _ = window.unminimize();
    window.show()?;
    window.set_focus()?;
    PERMISSION_GUIDE_WINDOW_READY.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn start_permission_guide_window_drag(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "drag permission guide window", |app| {
            if let Some(window) = app.get_webview_window(PERMISSION_GUIDE_WINDOW_LABEL) {
                window.start_dragging()?;
            }
            Ok(())
        })
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(window) = app.get_webview_window(PERMISSION_GUIDE_WINDOW_LABEL) {
        window.start_dragging()?;
    }
    #[cfg(not(target_os = "macos"))]
    Ok(())
}

fn ensure_permission_guide_window_on_main(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    if let Some(window) = app.get_webview_window(PERMISSION_GUIDE_WINDOW_LABEL) {
        if PERMISSION_GUIDE_WINDOW_READY.load(Ordering::SeqCst) {
            return show_permission_guide_window_on_main(app);
        }

        let created_at = PERMISSION_GUIDE_WINDOW_CREATED_AT_MS.load(Ordering::SeqCst);
        let age = current_unix_time_millis().saturating_sub(created_at);
        if created_at != 0 && age < 5_000 {
            return Ok(());
        }

        // A dynamically-created webview can survive a failed first navigation.
        // Showing that instance again only presents its blank backing window, so
        // discard it and give the permission guide a fresh navigation attempt.
        window.destroy()?;
    }

    if CREATING_PERMISSION_GUIDE_WINDOW.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    PERMISSION_GUIDE_WINDOW_READY.store(false, Ordering::SeqCst);
    PERMISSION_GUIDE_WINDOW_CREATED_AT_MS.store(current_unix_time_millis(), Ordering::SeqCst);

    let mut builder = WebviewWindowBuilder::new(
        app,
        PERMISSION_GUIDE_WINDOW_LABEL,
        WebviewUrl::App("permission-guide.html".into()),
    )
    .title("ArcRelay 权限引导")
    .inner_size(430.0, 800.0)
    .min_inner_size(430.0, 800.0)
    .max_inner_size(430.0, 800.0)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(false)
    .accept_first_mouse(true)
    .shadow(true)
    .visible(false)
    .center();

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .decorations(true)
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay);
    }

    let result = builder.build().map(|window| {
        position_permission_guide_at_screen_edge(&window);
    });
    CREATING_PERMISSION_GUIDE_WINDOW.store(false, Ordering::SeqCst);
    result
}

fn current_unix_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn position_permission_guide_at_screen_edge(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.primary_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let monitor_size = monitor.size().to_logical::<f64>(scale);
    let monitor_position = monitor.position().to_logical::<f64>(scale);
    let x = monitor_position.x + monitor_size.width - 430.0 - 24.0;
    let y = monitor_position.y + ((monitor_size.height - 800.0) / 2.0).max(24.0);
    let _ = window.set_position(Position::Logical(LogicalPosition::new(x, y)));
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct NativeWindowBounds {
    x: f64,
    y: f64,
    width: f64,
}

#[cfg(target_os = "macos")]
fn system_settings_window_bounds() -> Option<NativeWindowBounds> {
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::display::{
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
    };
    use core_graphics::geometry::CGRect;
    use objc2_app_kit::NSRunningApplication;
    use std::ffi::c_void;

    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *const c_void;
        fn CGRectMakeWithDictionaryRepresentation(
            dictionary: CFDictionaryRef,
            rect: *mut CGRect,
        ) -> bool;
    }

    unsafe fn value(dict: CFDictionaryRef, key: &str) -> Option<CFType> {
        let key = CFString::new(key);
        let raw = CFDictionaryGetValue(dict, key.as_CFTypeRef());
        (!raw.is_null()).then(|| CFType::wrap_under_get_rule(raw as CFStringRef as *const c_void))
    }

    unsafe fn integer(dict: CFDictionaryRef, key: &str) -> Option<i64> {
        value(dict, key).and_then(|value| {
            CFNumber::wrap_under_get_rule(value.as_CFTypeRef() as *const _).to_i64()
        })
    }

    let options = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
    let raw = unsafe { CGWindowListCopyWindowInfo(options, 0) };
    if raw.is_null() {
        return None;
    }
    let windows: CFArray = unsafe { CFArray::wrap_under_create_rule(raw as *const _) };
    for pointer in windows.get_all_values() {
        let dict = pointer as CFDictionaryRef;
        if unsafe { integer(dict, "kCGWindowLayer") } != Some(0) {
            continue;
        }
        let Some(pid) = (unsafe { integer(dict, "kCGWindowOwnerPID") }) else {
            continue;
        };
        let Some(application) =
            NSRunningApplication::runningApplicationWithProcessIdentifier(i32::try_from(pid).ok()?)
        else {
            continue;
        };
        if application
            .bundleIdentifier()
            .as_deref()
            .map(|value| value.to_string())
            != Some("com.apple.systempreferences".to_string())
        {
            continue;
        }
        let bounds_value = unsafe { value(dict, "kCGWindowBounds") }?;
        let mut rect = CGRect::new(
            &core_graphics::geometry::CGPoint::new(0.0, 0.0),
            &core_graphics::geometry::CGSize::new(0.0, 0.0),
        );
        if unsafe {
            CGRectMakeWithDictionaryRepresentation(
                bounds_value.as_CFTypeRef() as CFDictionaryRef,
                &mut rect,
            )
        } && rect.size.width > 300.0
            && rect.size.height > 300.0
        {
            return Some(NativeWindowBounds {
                x: rect.origin.x,
                y: rect.origin.y,
                width: rect.size.width,
            });
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn position_permission_guide_next_to_bounds(
    window: &tauri::WebviewWindow,
    settings: NativeWindowBounds,
) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let monitor_size = monitor.size().to_logical::<f64>(scale);
    let monitor_position = monitor.position().to_logical::<f64>(scale);
    let guide_width = 430.0;
    let guide_height = 800.0;
    let gap = 18.0;
    let inset = 14.0;
    let left = settings.x - guide_width - gap;
    let right = settings.x + settings.width + gap;
    let min_x = monitor_position.x + inset;
    let max_x = monitor_position.x + monitor_size.width - guide_width - inset;
    let left_overflow = (min_x - left).max(0.0);
    let right_overflow = (right - max_x).max(0.0);
    let x = if left_overflow <= right_overflow {
        left
    } else {
        right
    }
    .clamp(min_x, max_x);
    let max_y = monitor_position.y + monitor_size.height - guide_height - inset;
    let y = settings
        .y
        .max(monitor_position.y + inset)
        .min(max_y.max(monitor_position.y + inset));
    let _ = window.set_position(Position::Logical(LogicalPosition::new(x, y)));
}

pub fn schedule_permission_guide_next_to_system_settings(app: AppHandle) {
    #[cfg(target_os = "macos")]
    tauri::async_runtime::spawn(async move {
        for _ in 0..24 {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let bounds = tauri::async_runtime::spawn_blocking(system_settings_window_bounds)
                .await
                .ok()
                .flatten();
            let Some(bounds) = bounds else {
                continue;
            };
            if let Some(window) = app.get_webview_window(PERMISSION_GUIDE_WINDOW_LABEL) {
                let _ = dispatch_appkit(&app, "position permission guide window", move |_| {
                    position_permission_guide_next_to_bounds(&window, bounds);
                    Ok(())
                });
            }
            break;
        }
    });

    #[cfg(not(target_os = "macos"))]
    let _ = app;
}
