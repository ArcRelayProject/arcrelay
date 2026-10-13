//! A retained, independent transfer surface. Closing it never cancels backend work.
use super::*;
use std::sync::Mutex;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub const LABEL: &str = "tray-transfer";
#[cfg(target_os = "macos")]
const EVENT: &str = "tray-transfer-drop-changed";
#[cfg(target_os = "macos")]
const WIDTH: f64 = 440.0;

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum DropTarget {
    Tray,
    Panel,
}

#[derive(Default, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DropSnapshot {
    pub revision: u64,
    pub hovering: bool,
    pub paths: Vec<String>,
}

#[derive(Default)]
struct Session {
    drop: DropSnapshot,
    #[cfg(any(target_os = "macos", test))]
    target: Option<DropTarget>,
    has_content: bool,
    height: f64,
}

impl Session {
    #[cfg(any(target_os = "macos", test))]
    fn enter(&mut self, target: DropTarget) {
        self.drop.revision += 1;
        self.drop.hovering = true;
        self.target = Some(target);
    }

    #[cfg(any(target_os = "macos", test))]
    fn receive(&mut self, paths: Vec<String>) {
        self.drop.revision += 1;
        self.drop.hovering = false;
        self.target = None;
        self.drop.paths.extend(paths);
        self.has_content = true;
        self.height = 520.0;
    }

    fn take(&mut self) -> DropSnapshot {
        DropSnapshot {
            paths: std::mem::take(&mut self.drop.paths),
            ..self.drop.clone()
        }
    }

    #[cfg(any(target_os = "macos", test))]
    fn leave_revision(&self, target: DropTarget) -> Option<u64> {
        (self.target == Some(target)).then_some(self.drop.revision)
    }

    #[cfg(any(target_os = "macos", test))]
    fn leave(&mut self, revision: u64) -> bool {
        if self.drop.revision != revision {
            return false;
        }
        self.drop.hovering = false;
        self.target = None;
        self.drop.revision += 1;
        !self.has_content
    }
}

#[derive(Default)]
struct Runtime(Mutex<Session>);

#[cfg(any(target_os = "macos", test))]
async fn wait_for_drag_release(mut drag_state: impl FnMut() -> Option<bool>) -> bool {
    loop {
        match drag_state() {
            None => return false, // A newer enter/drop or explicit close superseded us.
            Some(false) => return true,
            Some(true) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
}

#[cfg(target_os = "macos")]
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    app.manage(Runtime::default());
    // The native drop target queues paths even before the WebView exists.
    macos::install(app)
}

#[cfg(target_os = "macos")]
fn ensure_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(LABEL).is_some() {
        return Ok(());
    }
    let window =
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("tray-transfer.html".into()))
            .title("ArcRelay")
            .inner_size(WIDTH, 220.0)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .resizable(false)
            .skip_taskbar(true)
            .accept_first_mouse(true)
            .focused(false)
            .visible(false)
            .build()?;
    macos::configure_panel(&window)?;
    let handle = app.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Enter { paths, .. })
            if !paths.is_empty() =>
        {
            enter(&handle, DropTarget::Panel)
        }
        tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => receive(
            &handle,
            paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        ),
        tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Leave) => {
            leave(&handle, DropTarget::Panel, true)
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = hide(&handle);
        }
        _ => {}
    });
    Ok(())
}

#[cfg(target_os = "macos")]
fn enter(app: &AppHandle, target: DropTarget) {
    app.state::<Runtime>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .enter(target);
    if let Err(error) = show(app, false) {
        tracing::warn!(%error, "Could not show file drop panel");
    }
    let _ = app.emit_to(LABEL, EVENT, ());
}

#[cfg(target_os = "macos")]
fn receive(app: &AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    app.state::<Runtime>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .receive(paths);
    let _ = show(app, true);
    let _ = app.emit_to(LABEL, EVENT, ());
}

#[cfg(target_os = "macos")]
fn leave(app: &AppHandle, target: DropTarget, wait_for_release: bool) {
    let Some(revision) = app
        .state::<Runtime>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .leave_revision(target)
    else {
        // AppKit may report the icon's exit after the panel's enter.
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Keep the destination available for the whole drag, including a slow
        // crossing of the gap or a detour outside the panel. AppKit's explicit
        // draggingEnded callback skips this wait (e.g. Escape with mouse held).
        let released = wait_for_drag_release(|| {
            let current = app
                .state::<Runtime>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .drop
                .revision
                == revision;
            current.then(|| wait_for_release && macos::left_mouse_button_down())
        })
        .await;
        if !released {
            return;
        }
        // Allow the destination's drop callback to arrive after mouse release.
        // A later enter/drop invalidates this leave, even from another drag.
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let dismiss = handle
                .state::<Runtime>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .leave(revision);
            if dismiss {
                let _ = hide(&handle);
            }
            let _ = handle.emit_to(LABEL, EVENT, ());
        });
    });
}

pub fn take_drop(app: &AppHandle) -> DropSnapshot {
    app.try_state::<Runtime>()
        .map(|state| state.0.lock().unwrap_or_else(|e| e.into_inner()).take())
        .unwrap_or_default()
}

pub fn reopen(app: &AppHandle) -> bool {
    let has_content = app.try_state::<Runtime>().is_some_and(|state| {
        state
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .has_content
    });
    has_content && show(app, true).is_ok()
}

fn show(app: &AppHandle, focus: bool) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    return dispatch_appkit(app, "show tray transfer", move |app| {
        ensure_window(&app)?;
        macos::position(&app)?;
        macos::show(&app, focus)
    });
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, focus);
        Ok(())
    }
}

pub fn hide(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    return dispatch_appkit(app, "hide tray transfer", |app| {
        {
            let runtime = app.state::<Runtime>();
            let mut session = runtime.0.lock().unwrap_or_else(|e| e.into_inner());
            let revision = session.drop.revision;
            session.leave(revision);
        }
        macos::hide(&app)
    });
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(())
    }
}

pub fn update_panel(app: &AppHandle, height: f64, has_content: bool) -> Result<(), String> {
    if !height.is_finite() {
        return Err("invalid panel height".into());
    }
    if let Some(state) = app.try_state::<Runtime>() {
        let mut session = state.0.lock().unwrap_or_else(|e| e.into_inner());
        session.height = height.clamp(170.0, 640.0);
        // A late frontend update must not erase a queued native drop.
        session.has_content = has_content || !session.drop.paths.is_empty();
    }
    #[cfg(target_os = "macos")]
    dispatch_appkit(app, "resize tray transfer", |app| macos::position(&app))
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn open_history(app: &AppHandle) -> Result<(), String> {
    super::tray::navigate(app, "transfers")?;
    hide(app).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn slow_drag_waits_for_release_instead_of_timing_out() {
        let started = tokio::time::Instant::now();
        assert!(
            wait_for_drag_release(|| {
                // The cursor takes two seconds to cross from the icon to the panel.
                Some(started.elapsed() < std::time::Duration::from_secs(2))
            })
            .await
        );
        assert!(started.elapsed() >= std::time::Duration::from_secs(2));
    }

    #[tokio::test(start_paused = true)]
    async fn newer_event_stops_waiting_even_while_mouse_is_held() {
        let started = tokio::time::Instant::now();
        assert!(
            !wait_for_drag_release(|| {
                (started.elapsed() < std::time::Duration::from_millis(200)).then_some(true)
            })
            .await
        );
        assert_eq!(started.elapsed(), std::time::Duration::from_millis(200));
    }

    #[test]
    fn drop_before_webview_ready_is_delivered_once_without_tokio() {
        let runtime = Runtime::default();
        let mut state = runtime.0.lock().unwrap();
        state.enter(DropTarget::Tray);
        let old_leave = state.drop.revision;
        state.receive(vec!["/tmp/first.txt".into()]);
        state.receive(vec!["/tmp/second.txt".into()]);
        assert!(!state.leave(old_leave));
        assert_eq!(state.take().paths, ["/tmp/first.txt", "/tmp/second.txt"]);
        assert!(state.take().paths.is_empty());
        assert!(state.has_content);
    }
    #[test]
    fn crossing_from_icon_to_panel_invalidates_old_leave() {
        let mut state = Session::default();
        state.enter(DropTarget::Tray);
        let icon_exit = state.drop.revision;
        state.enter(DropTarget::Panel);
        assert!(!state.leave(icon_exit));
        assert!(state.drop.hovering);
        assert!(state.leave(state.drop.revision));
        assert!(!state.drop.hovering);
    }

    #[test]
    fn delayed_icon_exit_cannot_schedule_a_leave_for_the_panel() {
        let mut state = Session::default();
        state.enter(DropTarget::Tray);
        state.enter(DropTarget::Panel);
        assert!(state.leave_revision(DropTarget::Tray).is_none());
        assert_eq!(
            state.leave_revision(DropTarget::Panel),
            Some(state.drop.revision)
        );
    }

    #[test]
    fn cancelled_drag_dismisses_empty_panel_but_keeps_existing_content() {
        let mut state = Session::default();
        state.enter(DropTarget::Tray);
        assert!(state.leave(state.drop.revision));
        assert!(state.leave_revision(DropTarget::Tray).is_none());

        state.receive(vec!["/tmp/first.txt".into()]);
        state.take();
        state.enter(DropTarget::Panel);
        assert!(!state.leave(state.drop.revision));
        assert!(!state.drop.hovering);
        assert!(state.has_content);
    }

    #[test]
    fn new_drag_invalidates_previous_cancellation() {
        let mut state = Session::default();
        state.enter(DropTarget::Panel);
        let old_leave = state.leave_revision(DropTarget::Panel).unwrap();
        state.enter(DropTarget::Tray);
        assert!(!state.leave(old_leave));
        assert!(state.drop.hovering);
    }
}
