use super::*;
use crate::clipboard_drag::{
    ClipboardDragMode, ClipboardDragPrepared, ClipboardDragSelection, ClipboardDragService,
};
use std::sync::Arc;
use tauri::WebviewWindow;

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_prepare_drag(
    state: State<'_, DesktopState>,
    service: State<'_, Arc<ClipboardDragService>>,
    window: WebviewWindow,
    ids: Vec<u64>,
    mode: ClipboardDragMode,
    selection: Option<ClipboardDragSelection>,
) -> Result<ClipboardDragPrepared, String> {
    service.prepare(&state, &window, ids, mode, selection).await
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_start_drag(
    state: State<'_, DesktopState>,
    service: State<'_, Arc<ClipboardDragService>>,
    window: WebviewWindow,
    token: String,
) -> Result<(), String> {
    service.start(&state, &window, token).await
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_cancel_drag(
    service: State<'_, Arc<ClipboardDragService>>,
    window: WebviewWindow,
    token: String,
) -> Result<(), String> {
    service.cancel(&window, &token)
}

#[cfg(test)]
include!(concat!(
    env!("OUT_DIR"),
    "/src_commands_clipboard_drag_ipc.rs"
));
