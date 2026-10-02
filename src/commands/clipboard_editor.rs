use super::*;
use arcrelay_core::domain::clipboard::{
    ClipboardPayload, ClipboardTextFormat, ClipboardTextPreview,
};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const TEXT_LIMIT: usize = 1024 * 1024;
const IMAGE_LIMIT: usize = 20 * 1024 * 1024;

#[derive(Default)]
pub struct ClipboardEditorSessions {
    sessions: Mutex<HashMap<String, Arc<tokio::sync::Mutex<EditorSession>>>>,
    opening: tokio::sync::Mutex<()>,
    hidden_for_lock: Mutex<HashSet<String>>,
}
struct EditorSession {
    token: String,
    source_id: u64,
    snapshot: ClipboardEditorSnapshot,
    saved_id: Option<u64>,
}
#[derive(Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardEditorSnapshot {
    source_id: u64,
    kind: String,
    text: Option<String>,
    image: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    plain_copy: bool,
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClipboardEditorDraft {
    Text { content: String },
    Image { png: String },
}
impl ClipboardEditorSessions {
    fn session(
        &self,
        window: &WebviewWindow,
    ) -> Result<Arc<tokio::sync::Mutex<EditorSession>>, String> {
        self.sessions
            .lock()
            .map_err(|_| "editor sessions are unavailable")?
            .get(window.label())
            .cloned()
            .ok_or_else(|| "editor session is unavailable".into())
    }
    pub fn reconcile_access(&self, app: &AppHandle, locked: bool) {
        let Ok(mut hidden) = self.hidden_for_lock.lock() else {
            return;
        };
        if locked {
            for (label, window) in app.webview_windows() {
                if label.starts_with("clipboard-editor-")
                    && window.is_visible().unwrap_or(false)
                    && window.hide().is_ok()
                {
                    hidden.insert(label);
                }
            }
        } else {
            for label in hidden.drain() {
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.show();
                }
            }
        }
    }
    pub fn remove(&self, label: &str) {
        if let Ok(mut hidden) = self.hidden_for_lock.lock() {
            hidden.remove(label);
        }
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(label);
        }
    }
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_editor_open(
    app: AppHandle,
    state: State<'_, DesktopState>,
    sessions: State<'_, ClipboardEditorSessions>,
    id: u64,
    plain_copy: bool,
) -> Result<(), String> {
    crate::presence_access::require_clipboard_access(&state)?;
    let _opening = sessions.opening.lock().await;
    let label = format!("clipboard-editor-{id}");
    if let Some(window) = app.get_webview_window(&label) {
        window.unminimize().map_err(|e| e.to_string())?;
        window.show().map_err(|e| e.to_string())?;
        return window.set_focus().map_err(|e| e.to_string());
    }
    if sessions
        .sessions
        .lock()
        .map_err(|_| "editor sessions are unavailable")?
        .len()
        >= 8
    {
        return Err("close an editor before opening another (maximum 8 windows)".into());
    }
    let mut records = state
        .clipboard
        .export_records(vec![id])
        .await
        .map_err(|e| e.to_string())?;
    let record = records.pop().ok_or("clipboard source is unavailable")?;
    let mut snapshot = ClipboardEditorSnapshot {
        source_id: id,
        kind: "text".into(),
        text: None,
        image: None,
        width: None,
        height: None,
        plain_copy: false,
    };
    match record.payload {
        ClipboardPayload::Text(text) => {
            snapshot.text = Some(text);
        }
        ClipboardPayload::RichText { plain_text, .. } if plain_copy => {
            snapshot.text = Some(plain_text);
            snapshot.plain_copy = true;
        }
        ClipboardPayload::RichText { .. } => {
            return Err("confirm editing a plain-text copy first".into())
        }
        ClipboardPayload::Image { png, width, height } => {
            if png.len() > IMAGE_LIMIT
                || u64::from(width) * u64::from(height) > 32 * 1024 * 1024
                || width == 0
                || height == 0
            {
                return Err("image exceeds the editor limit (20 MiB / 32 megapixels)".into());
            }
            snapshot.kind = "image".into();
            snapshot.width = Some(width);
            snapshot.height = Some(height);
            snapshot.image = Some(format!(
                "data:image/png;base64,{}",
                BASE64_STANDARD.encode(png)
            ));
        }
        _ => return Err("this clipboard record cannot be edited".into()),
    }
    if snapshot.text.as_ref().is_some_and(|s| s.len() > TEXT_LIMIT) {
        return Err("text exceeds the editor limit (1 MiB)".into());
    }
    crate::presence_access::require_clipboard_access(&state)?;
    sessions
        .sessions
        .lock()
        .map_err(|_| "editor sessions are unavailable")?
        .insert(
            label.clone(),
            Arc::new(tokio::sync::Mutex::new(EditorSession {
                token: uuid::Uuid::new_v4().to_string(),
                source_id: id,
                snapshot,
                saved_id: None,
            })),
        );
    let result = WebviewWindowBuilder::new(
        &app,
        &label,
        WebviewUrl::App("clipboard-editor.html".into()),
    )
    .title("ArcRelay · Clipboard editor")
    .inner_size(1100., 760.)
    .min_inner_size(720., 560.)
    .resizable(true)
    .always_on_top(false)
    .center()
    .build();
    if let Err(error) = result {
        sessions.remove(&label);
        return Err(error.to_string());
    }
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_editor_snapshot(
    state: State<'_, DesktopState>,
    sessions: State<'_, ClipboardEditorSessions>,
    window: WebviewWindow,
) -> Result<ClipboardEditorSnapshot, String> {
    crate::presence_access::require_clipboard_access(&state)?;
    let session = sessions.session(&window)?;
    let snapshot = session.lock().await.snapshot.clone();
    crate::presence_access::require_clipboard_access(&state)?;
    Ok(snapshot)
}

#[arcrelay_desktop_ipc::command]
pub fn clipboard_editor_access(state: State<'_, DesktopState>) -> Result<(), String> {
    crate::presence_access::require_clipboard_access(&state)
}

#[arcrelay_desktop_ipc::command]
pub fn clipboard_editor_preview(
    state: State<'_, DesktopState>,
    source: String,
    format: ClipboardTextFormat,
) -> Result<ClipboardTextPreview, String> {
    crate::presence_access::require_clipboard_access(&state)?;
    if source.len() > TEXT_LIMIT {
        return Err("text exceeds the editor limit (1 MiB)".into());
    }
    Ok(arcrelay_core::infrastructure::clipboard::preview_draft(
        source,
        false,
        Some(format),
    ))
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_editor_save(
    state: State<'_, DesktopState>,
    sessions: State<'_, ClipboardEditorSessions>,
    window: WebviewWindow,
    draft: ClipboardEditorDraft,
) -> Result<u64, String> {
    crate::presence_access::require_clipboard_access(&state)?;
    let session = sessions.session(&window)?;
    let mut session = session.lock().await;
    // A committed session is immutable. A repeated IPC response cannot create another entry.
    if let Some(id) = session.saved_id {
        return Ok(id);
    }
    let payload = match draft {
        ClipboardEditorDraft::Text { content } if session.snapshot.kind == "text" => {
            if content.is_empty() || content.len() > TEXT_LIMIT {
                return Err("edited text must be between 1 byte and 1 MiB".into());
            }
            ClipboardPayload::Text(content)
        }
        ClipboardEditorDraft::Image { png } if session.snapshot.kind == "image" => {
            if png.len() > IMAGE_LIMIT * 4 / 3 + 4 {
                return Err("edited image exceeds 20 MiB".into());
            }
            let png = BASE64_STANDARD
                .decode(png)
                .map_err(|_| "invalid PNG data")?;
            let _work = state
                .modules
                .content_resources()
                .work(256 * 1024 * 1024)
                .await
                .map_err(|e| e.to_string())?;
            let payload = tauri::async_runtime::spawn_blocking(
                move || -> Result<ClipboardPayload, String> {
                    let reader =
                        image::ImageReader::with_format(Cursor::new(&png), image::ImageFormat::Png);
                    let (width, height) = reader.into_dimensions().map_err(|e| e.to_string())?;
                    if width == 0
                        || height == 0
                        || width > 16384
                        || height > 16384
                        || u64::from(width) * u64::from(height) > 32 * 1024 * 1024
                    {
                        return Err("edited image exceeds 32 megapixels".into());
                    }
                    // Decode to reject truncated/corrupt data before committing. Preserve alpha and original pixels.
                    image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                        .map_err(|e| e.to_string())?;
                    Ok(ClipboardPayload::Image { png, width, height })
                },
            )
            .await
            .map_err(|e| e.to_string())??;
            payload
        }
        _ => return Err("draft type does not match the editor session".into()),
    };
    crate::presence_access::require_clipboard_access(&state)?;
    let id = state
        .clipboard
        .save_edited(session.token.clone(), session.source_id, payload)
        .await
        .map_err(|e| e.to_string())?;
    session.saved_id = Some(id);
    Ok(id)
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_editor_copy(
    state: State<'_, DesktopState>,
    sessions: State<'_, ClipboardEditorSessions>,
    window: WebviewWindow,
) -> Result<(), String> {
    crate::presence_access::require_clipboard_access(&state)?;
    let session = sessions.session(&window)?;
    let session = session.lock().await;
    let id = session.saved_id.ok_or("save the draft before copying")?;
    let _action = super::clipboard::begin_clipboard_action()?;
    state
        .clipboard
        .copy_edited(id)
        .await
        .map_err(|e| e.to_string())
}

#[arcrelay_desktop_ipc::command]
pub fn clipboard_editor_close(
    sessions: State<'_, ClipboardEditorSessions>,
    window: WebviewWindow,
) -> Result<(), String> {
    // The UI confirms dirty drafts; destruction releases the webview and all session memory.
    window.destroy().map_err(|e| e.to_string())?;
    sessions.remove(window.label());
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_edit_origins(
    state: State<'_, DesktopState>,
    ids: Vec<u64>,
) -> Result<Vec<(u64, u64)>, String> {
    crate::presence_access::require_clipboard_access(&state)?;
    state
        .clipboard
        .edit_origins(ids)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
include!(concat!(
    env!("OUT_DIR"),
    "/src_commands_clipboard_editor_ipc.rs"
));

#[cfg(test)]
mod tests {
    #[test]
    fn session_registry_constructs_without_async_runtime() {
        let registry = super::ClipboardEditorSessions::default();
        assert!(registry.sessions.lock().unwrap().is_empty());
    }
}
