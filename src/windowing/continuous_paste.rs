use super::*;
use crate::commands::ContinuousPasteProgress;
use crate::settings::ThemePreference;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

pub const CONTINUOUS_PASTE_HUD_LABEL: &str = "continuous-paste-hud";
pub(super) const HUD_WIDTH: f64 = 320.0;
pub(super) const HUD_HEIGHT: f64 = 92.0;
const HUD_EDGE_MARGIN: f64 = 24.0;

static HUD_VISIBILITY_REVISION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub(super) struct HudModel {
    pub title: String,
    pub position: String,
    pub preview: String,
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub stop_label: String,
    pub current: usize,
    pub total: usize,
    pub complete: bool,
    pub theme: ThemePreference,
}

fn model(app: &AppHandle, progress: &ContinuousPasteProgress) -> HudModel {
    let settings = app.state::<DesktopState>().settings.snapshot();
    let language = settings.language;
    let complete = !progress.active && progress.total > 0;
    let display_index = if progress.total == 0 {
        0
    } else if progress.active {
        (progress.current + 1).min(progress.total)
    } else {
        progress.current.min(progress.total)
    };
    let title = crate::localization::translate(
        language,
        if complete {
            "连续粘贴已完成"
        } else {
            "连续粘贴"
        },
        if complete {
            "Paste sequence complete"
        } else {
            "Paste sequence"
        },
    );
    let next = crate::localization::translate(
        language,
        "下一项 {current} / {total}",
        "Next {current} / {total}",
    )
    .replace("{current}", &display_index.to_string())
    .replace("{total}", &progress.total.to_string());
    let position = if complete {
        String::new()
    } else if progress.trigger_shortcut.is_empty() {
        next
    } else {
        format!("{} · {next}", progress.trigger_shortcut)
    };
    let preview = progress.next_preview.clone().unwrap_or_else(|| {
        crate::localization::translate(language, "全部项目均已粘贴", "All items pasted")
    });
    let stop_label =
        crate::localization::translate(language, "停止连续粘贴", "Stop paste sequence");

    HudModel {
        title,
        position,
        preview,
        stop_label,
        current: display_index,
        total: progress.total,
        complete,
        theme: settings.theme,
    }
}

pub fn show_continuous_paste_hud(
    app: &AppHandle,
    progress: &ContinuousPasteProgress,
) -> Result<(), String> {
    HUD_VISIBILITY_REVISION.fetch_add(1, Ordering::SeqCst);
    let model = model(app, progress);
    dispatch_native_ui(app, "show continuous paste HUD", move |app| {
        let window = ensure_window(&app)?;
        platform::update(&window, &model)?;
        position_hud(&app, &window);
        platform::show(&window)
    })
}

pub fn update_continuous_paste_hud(
    app: &AppHandle,
    progress: &ContinuousPasteProgress,
) -> Result<(), String> {
    let model = model(app, progress);
    dispatch_native_ui(app, "update continuous paste HUD", move |app| {
        let window = ensure_window(&app)?;
        platform::update(&window, &model)
    })
}

pub fn hide_continuous_paste_hud(app: &AppHandle) -> Result<(), String> {
    HUD_VISIBILITY_REVISION.fetch_add(1, Ordering::SeqCst);
    dispatch_native_ui(app, "hide continuous paste HUD", move |app| {
        if let Some(window) = app.get_window(CONTINUOUS_PASTE_HUD_LABEL) {
            platform::hide(&window)?;
        }
        Ok(())
    })
}

fn ensure_window(app: &AppHandle) -> Result<tauri::Window, String> {
    if let Some(window) = app.get_window(CONTINUOUS_PASTE_HUD_LABEL) {
        return Ok(window);
    }
    let builder = tauri::window::WindowBuilder::new(app, CONTINUOUS_PASTE_HUD_LABEL)
        .title("ArcRelay Continuous Paste")
        .inner_size(HUD_WIDTH, HUD_HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .decorations(false)
        .shadow(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false);
    #[cfg(target_os = "windows")]
    let builder = builder.transparent(false);
    #[cfg(not(target_os = "windows"))]
    let builder = builder.transparent(true);
    let window = builder
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;
    platform::install(app, &window)?;
    Ok(window)
}

fn dispatch_native_ui<T: Send + 'static>(
    app: &AppHandle,
    operation: &'static str,
    task: impl FnOnce(AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    #[cfg(target_os = "macos")]
    if objc2::MainThreadMarker::new().is_some() {
        return task(app.clone());
    }

    let app = app.clone();
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    app.clone()
        .run_on_main_thread(move || {
            let _ = result_tx.send(task(app));
        })
        .map_err(|error| error.to_string())?;
    result_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| format!("{operation} main-thread dispatch timed out: {error}"))?
}

fn position_hud(app: &AppHandle, window: &tauri::Window) {
    let cursor = app.cursor_position().ok();
    let monitor = cursor
        .and_then(|cursor| app.monitor_from_point(cursor.x, cursor.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        let _ = window.center();
        return;
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let width = HUD_WIDTH * scale;
    let margin = HUD_EDGE_MARGIN * scale;
    let x = f64::from(area.position.x) + f64::from(area.size.width) - width - margin;
    let y = f64::from(area.position.y) + margin;
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(
        x.round() as i32,
        y.round() as i32,
    )));
}

pub fn schedule_continuous_paste_hud_hide(app: AppHandle) {
    let revision = HUD_VISIBILITY_REVISION.load(Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
        if HUD_VISIBILITY_REVISION.load(Ordering::SeqCst) != revision
            || crate::commands::clipboard_continuous_paste_active()
        {
            return;
        }
        if let Err(error) = hide_continuous_paste_hud(&app) {
            tracing::warn!(%error, "Failed to hide completed continuous paste HUD");
        }
    });
}
