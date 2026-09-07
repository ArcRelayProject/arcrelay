#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    App, AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, Position,
    Size, WebviewUrl, WebviewWindowBuilder,
};

use crate::backend::DesktopState;

mod clipboard;
mod permission_guide;
mod tray;
pub(crate) mod tray_transfer;

pub use clipboard::*;
pub use permission_guide::*;
pub use tray::*;

#[cfg(target_os = "macos")]
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt as PanelManagerExt, StyleMask, WebviewWindowExt,
};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(ClipboardPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false
        }
    })
}

static CREATING_WINDOW: AtomicBool = AtomicBool::new(false);
static CREATING_CLIPBOARD_WINDOW: AtomicBool = AtomicBool::new(false);
static CREATING_PERMISSION_GUIDE_WINDOW: AtomicBool = AtomicBool::new(false);
static PERMISSION_GUIDE_WINDOW_READY: AtomicBool = AtomicBool::new(false);
static PERMISSION_GUIDE_WINDOW_CREATED_AT_MS: AtomicU64 = AtomicU64::new(0);
static CLIPBOARD_WINDOW_PINNED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static CLIPBOARD_PREVIOUS_FOREGROUND_WINDOW: AtomicIsize = AtomicIsize::new(0);
static CLIPBOARD_CONTEXT_MENU_OPEN: AtomicBool = AtomicBool::new(false);
static CLIPBOARD_SIZE_SAVE_REVISION: AtomicU64 = AtomicU64::new(0);
static CLIPBOARD_SCALE_RESTORE_REVISION: AtomicU64 = AtomicU64::new(0);
static MAIN_WINDOW_ACTIVE: AtomicBool = AtomicBool::new(false);
static MAIN_WINDOW_EXISTS: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static MAIN_DOCK_VISIBLE: AtomicBool = AtomicBool::new(true);
#[cfg(target_os = "macos")]
static MAIN_DOCK_VISIBILITY_REVISION: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "macos")]
static CLIPBOARD_OUTSIDE_CLICK_MONITOR_REVISION: AtomicU64 = AtomicU64::new(0);

pub const CLIPBOARD_WINDOW_LABEL: &str = "clipboard";
pub const PERMISSION_GUIDE_WINDOW_LABEL: &str = "permission-guide";
const CLIPBOARD_WINDOW_WIDTH: f64 = 540.0;
const CLIPBOARD_WINDOW_HEIGHT: f64 = 820.0;
const CLIPBOARD_WINDOW_MIN_WIDTH: f64 = 420.0;
const CLIPBOARD_WINDOW_MIN_HEIGHT: f64 = 560.0;
#[cfg(target_os = "macos")]
const CLIPBOARD_WINDOW_CORNER_RADIUS: f64 = 14.0;
const CLIPBOARD_WINDOW_CURSOR_OFFSET: f64 = 12.0;
const CLIPBOARD_WINDOW_EDGE_MARGIN: f64 = 12.0;
#[cfg(target_os = "macos")]
const CLIPBOARD_PANEL_LEVEL: i64 = 20;
#[cfg(target_os = "macos")]
const CLIPBOARD_OUTSIDE_CLICK_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(40);

#[derive(serde::Deserialize, serde::Serialize)]
struct ClipboardWindowSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
fn dispatch_appkit<T: Send + 'static>(
    app: &AppHandle,
    operation: &'static str,
    task: impl FnOnce(AppHandle) -> tauri::Result<T> + Send + 'static,
) -> tauri::Result<T> {
    const MAIN_THREAD_DISPATCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    let app = app.clone();
    if objc2::MainThreadMarker::new().is_some() {
        return task(app);
    }
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    app.clone().run_on_main_thread(move || {
        debug_assert!(objc2::MainThreadMarker::new().is_some());
        let _ = result_tx.send(task(app));
    })?;
    result_rx
        .recv_timeout(MAIN_THREAD_DISPATCH_TIMEOUT)
        .map_err(|error| {
            tauri::Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("{operation} main-thread dispatch timed out: {error}"),
            ))
        })?
}

pub fn set_main_window_active(active: bool) {
    MAIN_WINDOW_ACTIVE.store(active, Ordering::SeqCst);
}

pub fn main_window_active() -> bool {
    MAIN_WINDOW_ACTIVE.load(Ordering::SeqCst)
}

pub fn set_main_window_exists(exists: bool) {
    MAIN_WINDOW_EXISTS.store(exists, Ordering::SeqCst);
    if !exists {
        set_main_window_active(false);
    }
}

#[cfg(target_os = "macos")]
pub fn main_window_exists() -> bool {
    MAIN_WINDOW_EXISTS.load(Ordering::SeqCst)
}

#[cfg(target_os = "macos")]
fn set_main_dock_visibility(app: &AppHandle, visible: bool) {
    let revision = MAIN_DOCK_VISIBILITY_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    let changed = MAIN_DOCK_VISIBLE.swap(visible, Ordering::SeqCst) != visible;
    if changed {
        if let Err(error) = app.set_dock_visibility(visible) {
            tracing::warn!(visible, %error, "failed to update ArcRelay Dock visibility");
        }
    }

    if visible {
        return;
    }

    // macOS ignores a Dock-hide request made within one second of showing the
    // icon. Retry once so quickly reopening and closing the window cannot leave
    // a stale running-app icon behind. The revision prevents an old retry from
    // hiding the icon after the main window has been reopened.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
        if MAIN_DOCK_VISIBILITY_REVISION.load(Ordering::SeqCst) != revision || main_window_exists()
        {
            return;
        }
        if let Err(error) = app.set_dock_visibility(false) {
            tracing::warn!(%error, "Failed to confirm hidden ArcRelay Dock icon");
        }
    });
}

pub fn main_window_destroyed(_app: &AppHandle) {
    set_main_window_exists(false);
    #[cfg(target_os = "macos")]
    set_main_dock_visibility(_app, false);
}

pub fn setup_main_window(app: &AppHandle) -> tauri::Result<()> {
    let settings = app.state::<DesktopState>().settings.snapshot();
    if !settings.launch_silently {
        return ensure_main_window(app);
    }

    // Leave the main webview uncreated until the user opens it from the tray.
    set_main_window_exists(false);
    #[cfg(target_os = "macos")]
    set_main_dock_visibility(app, false);
    Ok(())
}

pub fn ensure_main_window(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "ensure main window", |app| {
            ensure_main_window_on_main(&app)
        })
    }
    #[cfg(not(target_os = "macos"))]
    ensure_main_window_on_main(app)
}

fn ensure_main_window_on_main(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    #[cfg(target_os = "macos")]
    set_main_dock_visibility(app, true);
    if let Some(window) = app.get_webview_window("main") {
        set_main_window_exists(true);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        set_main_window_active(true);
        return Ok(());
    }

    if CREATING_WINDOW.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("ArcRelay")
        .inner_size(1180.0, 760.0)
        .min_inner_size(920.0, 620.0)
        .resizable(true)
        .shadow(true)
        .center();

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .decorations(true)
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay);
    }

    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.decorations(false);
    }

    let result = builder.build().map(|window| {
        set_main_window_exists(true);
        let _ = window.set_focus();
        set_main_window_active(true);
    });

    CREATING_WINDOW.store(false, Ordering::SeqCst);
    result
}

pub fn request_main_window(app: AppHandle) {
    if let Err(error) = ensure_main_window(&app) {
        tracing::error!(%error, "Failed to request ArcRelay window");
    }
}
