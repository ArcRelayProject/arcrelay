//! Local clipboard drag-out. Payloads stay in Rust; the WebView receives only
//! short-lived, window-bound tokens. Preparing a drag never activates a record.
mod cache;
mod html;
mod native;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use arcrelay_core::domain::clipboard::{
    ClipboardExportRecord, ClipboardPayload, CLIPBOARD_EXPORT_MAX_RECORDS,
};
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::backend::DesktopState;
use cache::{ExportFile, ExportLease};
use native::{NativeDragOutcome, NativeDragPayload};

const PREPARED_TTL: Duration = Duration::from_secs(120);
const MAX_PREPARED: usize = 4;
const MAX_FILES: usize = 512;
const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
const DRAG_ENDED: &str = "clipboard-drag-ended";
static ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, ts_rs::TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardDragMode {
    #[default]
    Auto,
    PlainText,
    RichText,
    TextFile,
}

#[derive(Debug, Clone, Deserialize, ts_rs::TS)]
pub struct ClipboardDragSelection {
    pub id: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct ClipboardDragPrepared {
    pub token: String,
    pub kind: ClipboardDragKind,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardDragKind {
    Text,
    Files,
}

#[derive(Debug, Clone, Copy, Serialize, ts_rs::TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardDragOutcome {
    Dropped,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct ClipboardDragEnded {
    pub token: String,
    pub outcome: ClipboardDragOutcome,
    pub error: Option<String>,
}

struct Prepared {
    owner: String,
    created: Instant,
    generation: u64,
    versions: Vec<(u64, String)>,
    payload: NativeDragPayload,
    lease: Option<ExportLease>,
}

struct ActiveDrag {
    token: String,
    owner: String,
    cancelled: Arc<AtomicBool>,
    lease: Option<ExportLease>,
    finishing: bool,
}

#[derive(Default)]
struct Sessions {
    prepared: HashMap<String, Prepared>,
    active: Option<ActiveDrag>,
}

pub struct ClipboardDragService {
    sessions: Mutex<Sessions>,
    cache_gate: Mutex<()>,
    slots: Arc<tokio::sync::Semaphore>,
    generation: AtomicU64,
    stop: tokio_util::sync::CancellationToken,
    maintenance: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl Default for ClipboardDragService {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(Sessions::default()),
            cache_gate: Mutex::new(()),
            slots: Arc::new(tokio::sync::Semaphore::new(2)),
            generation: AtomicU64::new(0),
            stop: tokio_util::sync::CancellationToken::new(),
            maintenance: Mutex::new(None),
        }
    }
}

pub fn is_dragging() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

fn require_access(state: &DesktopState) -> Result<(), String> {
    crate::presence_access::require_clipboard_access(state)?;
    if !state.settings.snapshot().clipboard_enabled {
        return Err("clipboard history is disabled".into());
    }
    Ok(())
}

fn require_owner(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != crate::windowing::CLIPBOARD_WINDOW_LABEL {
        return Err("clipboard dragging is only available in the clipboard window".into());
    }
    Ok(())
}

fn cache_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_cache_dir()
        .map(|root| root.join("clipboard-exports"))
        .map_err(|_| "clipboard export cache is unavailable".into())
}

impl ClipboardDragService {
    fn protected(&self) -> Vec<PathBuf> {
        let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions
            .prepared
            .values()
            .filter_map(|p| p.lease.as_ref().map(ExportLease::directory))
            .chain(
                sessions
                    .active
                    .iter()
                    .filter_map(|p| p.lease.as_ref().map(ExportLease::directory)),
            )
            .collect()
    }

    fn expire_prepared(&self) {
        let expired = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            let keys = sessions
                .prepared
                .iter()
                .filter(|(_, item)| item.created.elapsed() >= PREPARED_TTL)
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| sessions.prepared.remove(&key))
                .collect::<Vec<_>>()
        };
        drop(expired);
    }

    /// Called from setup, after the application's long-lived async runtime is
    /// installed. `default()` itself performs no IO and requires no runtime.
    pub fn start_maintenance(self: &Arc<Self>, app: &AppHandle) {
        let Ok(root) = cache_root(app) else { return };
        let mut handle = self.maintenance.lock().unwrap_or_else(|e| e.into_inner());
        if handle.is_some() {
            return;
        }
        let service = Arc::downgrade(self);
        let stop = self.stop.clone();
        *handle = Some(tauri::async_runtime::spawn(async move {
            loop {
                let Some(service) = service.upgrade() else {
                    break;
                };
                let root = root.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    service.expire_prepared();
                    let _gate = service.cache_gate.lock().unwrap_or_else(|e| e.into_inner());
                    let _ =
                        cache::collect(&root, std::time::SystemTime::now(), &service.protected());
                })
                .await;
                tokio::select! {
                    _ = stop.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(300)) => {},
                }
            }
        }));
    }

    pub async fn prepare(
        self: &Arc<Self>,
        state: &DesktopState,
        window: &WebviewWindow,
        ids: Vec<u64>,
        mode: ClipboardDragMode,
        selection: Option<ClipboardDragSelection>,
    ) -> Result<ClipboardDragPrepared, String> {
        require_owner(window)?;
        require_access(state)?;
        if ids.is_empty() || ids.len() > CLIPBOARD_EXPORT_MAX_RECORDS || is_dragging() {
            return Err("invalid clipboard drag selection or another drag is active".into());
        }
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| "clipboard drag preparation is busy")?;
        let generation = self.generation.load(Ordering::Acquire);
        let records = state
            .clipboard
            .export_records(ids)
            .await
            .map_err(|e| e.to_string())?;
        require_access(state)?;
        let versions = records
            .iter()
            .map(|r| (r.id, r.content_hash.clone()))
            .collect();
        let root = cache_root(window.app_handle())?;
        let service = self.clone();
        let (payload, lease) = tauri::async_runtime::spawn_blocking(move || {
            // Keep admission until the blocking IO completes, even if IPC is cancelled.
            let _permit = permit;
            let assembly = assemble(records, mode, selection)?;
            let _gate = service.cache_gate.lock().unwrap_or_else(|e| e.into_inner());
            let (generated, lease) =
                cache::materialize(&root, assembly.exports, &service.protected())?;
            let mut generated = generated.into_iter();
            let paths = assembly
                .files
                .into_iter()
                .map(|file| match file {
                    DragFile::Existing(path) => path,
                    DragFile::Export => generated.next().expect("one path for each export"),
                })
                .collect();
            Ok::<_, String>((
                NativeDragPayload {
                    files: paths,
                    text: assembly.text,
                    html: assembly.html,
                    preview_png: assembly.preview,
                    cancelled: Arc::new(AtomicBool::new(false)),
                    offered: lease
                        .as_ref()
                        .map(ExportLease::handoff_flag)
                        .unwrap_or_default(),
                },
                lease,
            ))
        })
        .await
        .map_err(|e| e.to_string())??;
        require_access(state)?;
        if self.generation.load(Ordering::Acquire) != generation {
            return Err("clipboard drag preparation was cancelled".into());
        }
        let result = ClipboardDragPrepared {
            token: uuid::Uuid::new_v4().to_string(),
            kind: if payload.files.is_empty() {
                ClipboardDragKind::Text
            } else {
                ClipboardDragKind::Files
            },
            count: payload.files.len().max(1),
        };
        self.expire_prepared();
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let oldest = if sessions.prepared.len() >= MAX_PREPARED {
            sessions
                .prepared
                .iter()
                .min_by_key(|(_, item)| item.created)
                .map(|(key, _)| key.clone())
        } else {
            None
        };
        let evicted = oldest.and_then(|key| sessions.prepared.remove(&key));
        sessions.prepared.insert(
            result.token.clone(),
            Prepared {
                owner: window.label().into(),
                created: Instant::now(),
                generation,
                versions,
                payload,
                lease,
            },
        );
        drop(sessions);
        drop(evicted);
        Ok(result)
    }

    pub async fn start(
        self: &Arc<Self>,
        state: &DesktopState,
        window: &WebviewWindow,
        token: String,
    ) -> Result<(), String> {
        require_owner(window)?;
        require_access(state)?;
        let versions = {
            let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            let prepared = sessions
                .prepared
                .get(&token)
                .ok_or("clipboard drag expired; drag again")?;
            if prepared.owner != window.label() || prepared.created.elapsed() >= PREPARED_TTL {
                return Err("clipboard drag expired; drag again".into());
            }
            prepared.versions.clone()
        };
        // Recheck external files just before native handoff, off the UI thread.
        let files = {
            let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            sessions
                .prepared
                .get(&token)
                .ok_or("clipboard drag was cancelled")?
                .payload
                .files
                .clone()
        };
        tauri::async_runtime::spawn_blocking(move || validate_files(files))
            .await
            .map_err(|e| e.to_string())??;
        if !state
            .clipboard
            .validate_export(versions)
            .await
            .map_err(|e| e.to_string())?
        {
            self.cancel(window, &token)?;
            return Err("clipboard content changed or was deleted; drag again".into());
        }
        require_access(state)?;
        let prepared = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            if sessions.active.is_some()
                || ACTIVE
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
            {
                return Err("another clipboard drag is active".into());
            }
            let Some(prepared) = sessions.prepared.remove(&token) else {
                ACTIVE.store(false, Ordering::Release);
                return Err("clipboard drag was cancelled".into());
            };
            if prepared.generation != self.generation.load(Ordering::Acquire)
                || prepared.created.elapsed() >= PREPARED_TTL
            {
                ACTIVE.store(false, Ordering::Release);
                return Err("clipboard drag expired; drag again".into());
            }
            sessions.active = Some(ActiveDrag {
                token: token.clone(),
                owner: prepared.owner.clone(),
                cancelled: prepared.payload.cancelled.clone(),
                lease: prepared.lease,
                finishing: false,
            });
            prepared.payload
        };
        let app = window.app_handle().clone();
        let native_window = window.clone();
        let service = self.clone();
        let failure_service = self.clone();
        let failure_app = app.clone();
        let failure_token = token.clone();
        if let Err(error) = window.run_on_main_thread(move || {
            // Authorization can change while the UI dispatch was queued.
            if let Some(state) = app.try_state::<DesktopState>() {
                if require_access(&state).is_err() {
                    service.finish(&app, &token, ClipboardDragOutcome::Cancelled, None);
                    return;
                }
            }
            let callback_service = service.clone();
            let callback_app = app.clone();
            let callback_token = token.clone();
            let result = native::start(
                &native_window,
                prepared,
                Box::new(move |outcome| {
                    callback_service.finish(
                        &callback_app,
                        &callback_token,
                        match outcome {
                            NativeDragOutcome::Dropped => ClipboardDragOutcome::Dropped,
                            NativeDragOutcome::Cancelled => ClipboardDragOutcome::Cancelled,
                        },
                        None,
                    );
                }),
            );
            if let Err(error) = result {
                service.finish(&app, &token, ClipboardDragOutcome::Failed, Some(error));
            }
        }) {
            failure_service.finish(
                &failure_app,
                &failure_token,
                ClipboardDragOutcome::Failed,
                Some(error.to_string()),
            );
            return Err(error.to_string());
        }
        Ok(())
    }

    pub fn cancel(&self, window: &WebviewWindow, token: &str) -> Result<(), String> {
        require_owner(window)?;
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let prepared = if sessions
            .prepared
            .get(token)
            .is_some_and(|p| p.owner == window.label())
        {
            sessions.prepared.remove(token)
        } else {
            None
        };
        if let Some(active) = sessions
            .active
            .as_ref()
            .filter(|p| p.token == token && p.owner == window.label())
        {
            active.cancelled.store(true, Ordering::Release);
        }
        drop(sessions);
        drop(prepared);
        Ok(())
    }

    fn finish(
        self: &Arc<Self>,
        app: &AppHandle,
        token: &str,
        outcome: ClipboardDragOutcome,
        error: Option<String>,
    ) {
        {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            let Some(active) = sessions
                .active
                .as_mut()
                .filter(|active| active.token == token && !active.finishing)
            else {
                return;
            };
            active.finishing = true;
        }
        let service = self.clone();
        let app = app.clone();
        let token = token.to_string();
        tauri::async_runtime::spawn(async move {
            let cleanup = service.clone();
            let cleanup_token = token.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                let _gate = cleanup.cache_gate.lock().unwrap_or_else(|e| e.into_inner());
                // Keep the active lease protected until its retention has been
                // extended from completion, even for a very long-running drag.
                let active = {
                    let mut sessions = cleanup.sessions.lock().unwrap_or_else(|e| e.into_inner());
                    if !sessions.active.as_ref().is_some_and(|active| active.token == cleanup_token) { return; }
                    sessions.active.take()
                };
                if let Some(active) = active {
                    if let Some(lease) = &active.lease {
                        if let Err(error) = lease.retain_after_handoff() {
                            tracing::warn!(event = "clipboard.drag.retention_failed", %error, "could not extend drag export retention");
                        }
                    }
                    drop(active);
                }
                ACTIVE.store(false, Ordering::Release);
            }).await;
            let _ = app.emit_to(
                crate::windowing::CLIPBOARD_WINDOW_LABEL,
                DRAG_ENDED,
                ClipboardDragEnded {
                    token,
                    outcome,
                    error,
                },
            );
            // The native adapter has already unwound its final callback. This
            // dispatch originates off the UI thread, after file lease cleanup.
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                let disabled = handle
                    .try_state::<DesktopState>()
                    .is_some_and(|state| !state.settings.snapshot().clipboard_enabled);
                let locked = handle
                    .try_state::<DesktopState>()
                    .is_some_and(|state| crate::presence_access::clipboard_locked(&state));
                if disabled {
                    let _ = crate::windowing::sync_clipboard_window(&handle, false);
                } else if locked
                    || (outcome == ClipboardDragOutcome::Dropped
                        && !crate::windowing::clipboard_window_pinned())
                {
                    let _ = crate::windowing::hide_clipboard_window(&handle);
                }
            });
        });
    }

    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let prepared = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(active) = &sessions.active {
                active.cancelled.store(true, Ordering::Release);
            }
            std::mem::take(&mut sessions.prepared)
        };
        if !prepared.is_empty() {
            tauri::async_runtime::spawn_blocking(move || drop(prepared));
        }
    }
}

pub fn invalidate(app: &AppHandle) {
    if let Some(service) = app.try_state::<Arc<ClipboardDragService>>() {
        service.invalidate();
    }
}

pub fn shutdown(app: &AppHandle) {
    if let Some(service) = app.try_state::<Arc<ClipboardDragService>>() {
        service.invalidate();
        service.stop.cancel();
        if let Some(task) = service
            .maintenance
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            task.abort();
        }
    }
}

enum DragFile {
    Existing(PathBuf),
    Export,
}
struct Assembly {
    files: Vec<DragFile>,
    exports: Vec<ExportFile>,
    text: Option<String>,
    html: Option<String>,
    preview: Vec<u8>,
}

fn validate_files(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    if paths.len() > MAX_FILES {
        return Err("too many clipboard files; select at most 512 files".into());
    }
    paths
        .into_iter()
        .map(|path| {
            let absolute = std::fs::canonicalize(path)
                .map_err(|_| "original clipboard file was moved, deleted or is unavailable")?;
            let metadata =
                std::fs::metadata(&absolute).map_err(|_| "clipboard file is unavailable")?;
            if metadata.is_file() {
                std::fs::File::open(&absolute).map_err(|_| "clipboard file cannot be read")?;
            } else if metadata.is_dir() {
                std::fs::read_dir(&absolute).map_err(|_| "clipboard folder cannot be read")?;
            } else {
                return Err("clipboard drag supports regular files and folders only".into());
            }
            Ok(absolute)
        })
        .collect()
}

fn escape_html(text: &str, output: &mut String, limit: usize) -> Result<(), String> {
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        let replacement = match byte {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            _ => continue,
        };
        html::append(output, &text[start..index], limit)?;
        html::append(output, replacement, limit)?;
        start = index + 1;
    }
    html::append(output, &text[start..], limit)
}

fn assemble(
    mut records: Vec<ClipboardExportRecord>,
    mode: ClipboardDragMode,
    selection: Option<ClipboardDragSelection>,
) -> Result<Assembly, String> {
    let selected = selection.is_some();
    if let Some(selection) = selection {
        if records.len() != 1
            || records[0].id != selection.id
            || selection.text.is_empty()
            || selection.text.len() > MAX_TEXT_BYTES
        {
            return Err("invalid clipboard text selection".into());
        }
        // The trusted local WebView supplies the browser selection, just like
        // paste-selected-text. Rendered Markdown/HTML may join noncontiguous
        // source ranges, so substring validation would reject valid selections.
        let valid = matches!(
            &records[0].payload,
            ClipboardPayload::Text(_) | ClipboardPayload::RichText { .. }
        );
        if !valid {
            return Err("clipboard text selection changed; select it again".into());
        }
        records[0].payload = ClipboardPayload::Text(selection.text);
    }
    let all_text = records.iter().all(|r| {
        matches!(
            r.payload,
            ClipboardPayload::Text(_) | ClipboardPayload::RichText { .. }
        )
    });
    let mode = if all_text {
        mode
    } else {
        ClipboardDragMode::Auto
    };
    let preview = if records.len() == 1 && !records[0].sensitive {
        match &records[0].payload {
            ClipboardPayload::Image { png, .. } => image_preview(png),
            _ => default_preview(),
        }
    } else {
        default_preview()
    };
    let mut result = Assembly {
        files: vec![],
        exports: vec![],
        text: None,
        html: None,
        preview,
    };
    if all_text {
        let offer_html = !selected
            && !matches!(
                mode,
                ClipboardDragMode::PlainText | ClipboardDragMode::TextFile
            )
            && (mode == ClipboardDragMode::RichText
                || records
                    .iter()
                    .any(|record| matches!(record.payload, ClipboardPayload::RichText { .. })));
        let single_record = records.len() == 1;
        let mut plain = String::new();
        let mut rich_output = String::new();
        if offer_html && !single_record {
            html::append(
                &mut rich_output,
                "<html><body><!--StartFragment-->",
                MAX_TEXT_BYTES,
            )?;
        }
        for (index, record) in records.into_iter().enumerate() {
            if index > 0 {
                html::append(&mut plain, "\n\n", MAX_TEXT_BYTES)?;
                if offer_html {
                    html::append(&mut rich_output, "<br><br>", MAX_TEXT_BYTES)?;
                }
            }
            match record.payload {
                ClipboardPayload::Text(text) => {
                    html::append(&mut plain, &text, MAX_TEXT_BYTES)?;
                    if offer_html {
                        html::append(&mut rich_output, "<pre>", MAX_TEXT_BYTES)?;
                        escape_html(&text, &mut rich_output, MAX_TEXT_BYTES)?;
                        html::append(&mut rich_output, "</pre>", MAX_TEXT_BYTES)?;
                    }
                }
                ClipboardPayload::RichText {
                    html: rich,
                    plain_text,
                    ..
                } => {
                    html::append(&mut plain, &plain_text, MAX_TEXT_BYTES)?;
                    if offer_html {
                        if rich.len() > MAX_TEXT_BYTES {
                            return Err("clipboard text export exceeds 16 MiB".into());
                        }
                        if single_record {
                            // Retain original document styles and CF_HTML context.
                            rich_output = rich;
                        } else {
                            let fragment =
                                html::fragment(&rich, MAX_TEXT_BYTES - rich_output.len())?;
                            html::append(&mut rich_output, &fragment, MAX_TEXT_BYTES)?;
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
        if offer_html && !single_record {
            html::append(
                &mut rich_output,
                "<!--EndFragment--></body></html>",
                MAX_TEXT_BYTES,
            )?;
        }
        if mode == ClipboardDragMode::TextFile {
            result.files.push(DragFile::Export);
            result.exports.push(ExportFile {
                extension: "txt",
                bytes: plain.into_bytes(),
            });
        } else {
            result.text = Some(plain);
            if offer_html {
                result.html = Some(rich_output);
            }
        }
    } else {
        let mut seen = HashSet::new();
        for record in records {
            match record.payload {
                ClipboardPayload::Files(paths) => {
                    if paths.is_empty() {
                        return Err("clipboard file list is empty".into());
                    }
                    for path in validate_files(paths.into_iter().map(PathBuf::from).collect())? {
                        if seen.insert(path.clone()) {
                            result.files.push(DragFile::Existing(path));
                        }
                    }
                }
                ClipboardPayload::Image { png, .. } => {
                    if png.is_empty() {
                        return Err("clipboard image is empty".into());
                    }
                    result.files.push(DragFile::Export);
                    result.exports.push(ExportFile {
                        extension: "png",
                        bytes: png,
                    });
                }
                ClipboardPayload::Text(text)
                | ClipboardPayload::RichText {
                    plain_text: text, ..
                } => {
                    if text.len() > MAX_TEXT_BYTES {
                        return Err("clipboard text export exceeds 16 MiB".into());
                    }
                    result.files.push(DragFile::Export);
                    result.exports.push(ExportFile {
                        extension: "txt",
                        bytes: text.into_bytes(),
                    });
                }
            }
            if result.files.len() > MAX_FILES {
                return Err("too many clipboard files; select at most 512 files".into());
            }
        }
    }
    Ok(result)
}

fn default_preview() -> Vec<u8> {
    include_bytes!("../../icons/icon-drag-preview.png").to_vec()
}

fn image_preview(png: &[u8]) -> Vec<u8> {
    // The preview has its own bounded decode; export always retains original bytes.
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(png), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(32 * 1024 * 1024);
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    reader.limits(limits);
    let Ok(image) = reader.decode() else {
        return default_preview();
    };
    let thumbnail = image.thumbnail(128, 128).into_rgba8();
    let mut output = Vec::new();
    if image::codecs::png::PngEncoder::new(&mut output)
        .write_image(
            thumbnail.as_raw(),
            thumbnail.width(),
            thumbnail.height(),
            image::ExtendedColorType::Rgba8,
        )
        .is_err()
    {
        return default_preview();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(id: u64, payload: ClipboardPayload) -> ClipboardExportRecord {
        ClipboardExportRecord {
            id,
            content_hash: id.to_string(),
            sensitive: false,
            payload,
        }
    }
    #[test]
    fn service_construction_needs_no_tokio_runtime() {
        let service = ClipboardDragService::default();
        assert_eq!(service.slots.available_permits(), 2);
        assert!(service.maintenance.lock().unwrap().is_none());
        service.invalidate();
        assert_eq!(service.generation.load(Ordering::Acquire), 1);
    }
    #[test]
    fn text_modes_preserve_full_text_and_offer_html_only_when_requested() {
        let records = vec![
            record(1, ClipboardPayload::Text("first\nline".into())),
            record(
                2,
                ClipboardPayload::RichText {
                    html: "<b>中文</b>".into(),
                    plain_text: "中文".into(),
                    rtf: None,
                },
            ),
        ];
        let auto = assemble(records.clone(), ClipboardDragMode::Auto, None).unwrap();
        assert_eq!(auto.text.as_deref(), Some("first\nline\n\n中文"));
        assert!(auto.html.unwrap().contains("<b>中文</b>"));
        let plain = assemble(records.clone(), ClipboardDragMode::PlainText, None).unwrap();
        assert!(plain.html.is_none());
        let file = assemble(records, ClipboardDragMode::TextFile, None).unwrap();
        assert!(file.text.is_none());
        assert_eq!(file.exports[0].bytes, "first\nline\n\n中文".as_bytes());
    }
    #[test]
    fn multi_record_rich_text_has_one_document_and_one_complete_fragment() {
        let first = "<html><head><title>First</title></head><body><div class='one'><!--StartFragment--><b>第一条</b><!--EndFragment--></div></body></html>";
        let second = "<HTML><BODY title='a > b'><table><tbody><!--StartFragment--><tr><td>第二条</td></tr><!--EndFragment--></tbody></table></BODY></HTML>";
        let records = vec![
            record(
                1,
                ClipboardPayload::RichText {
                    html: first.into(),
                    plain_text: "第一条".into(),
                    rtf: None,
                },
            ),
            record(
                2,
                ClipboardPayload::RichText {
                    html: second.into(),
                    plain_text: "第二条".into(),
                    rtf: None,
                },
            ),
        ];
        let single = assemble(vec![records[0].clone()], ClipboardDragMode::Auto, None).unwrap();
        assert_eq!(single.html.as_deref(), Some(first));
        let combined = assemble(records, ClipboardDragMode::Auto, None).unwrap();
        assert_eq!(combined.text.as_deref(), Some("第一条\n\n第二条"));
        let combined = combined.html.unwrap();
        assert_eq!(combined.matches("<!--StartFragment-->").count(), 1);
        assert_eq!(combined.matches("<!--EndFragment-->").count(), 1);
        assert_eq!(combined.to_ascii_lowercase().matches("<html>").count(), 1);
        let start = combined.find("<!--StartFragment-->").unwrap() + "<!--StartFragment-->".len();
        let end = combined.find("<!--EndFragment-->").unwrap();
        assert_eq!(&combined[start..end], "<div class='one'><b>第一条</b></div><br><br><table><tbody><tr><td>第二条</td></tr></tbody></table>");
    }
    #[test]
    fn plain_and_txt_do_not_allocate_or_count_unused_escaped_html() {
        let large = "&".repeat(9 * 1024 * 1024);
        for mode in [
            ClipboardDragMode::Auto,
            ClipboardDragMode::PlainText,
            ClipboardDragMode::TextFile,
        ] {
            let result = assemble(
                vec![record(1, ClipboardPayload::Text(large.clone()))],
                mode,
                None,
            )
            .unwrap();
            assert!(result.html.is_none());
            if mode == ClipboardDragMode::TextFile {
                assert_eq!(result.exports[0].bytes, large.as_bytes());
            } else {
                assert_eq!(result.text.as_deref(), Some(large.as_str()));
            }
        }
        assert!(assemble(
            vec![record(1, ClipboardPayload::Text(large))],
            ClipboardDragMode::RichText,
            None
        )
        .is_err());
    }
    #[test]
    fn text_budget_counts_actual_joined_output_and_file_exports() {
        let boundary = "a".repeat(MAX_TEXT_BYTES);
        let result = assemble(
            vec![record(1, ClipboardPayload::Text(boundary.clone()))],
            ClipboardDragMode::TextFile,
            None,
        )
        .unwrap();
        assert_eq!(result.exports[0].bytes.len(), MAX_TEXT_BYTES);
        assert!(assemble(
            vec![
                record(1, ClipboardPayload::Text(boundary)),
                record(2, ClipboardPayload::Text("b".into()))
            ],
            ClipboardDragMode::PlainText,
            None
        )
        .is_err());
        assert!(assemble(
            vec![
                record(1, ClipboardPayload::Text("a".repeat(MAX_TEXT_BYTES + 1))),
                record(
                    2,
                    ClipboardPayload::Image {
                        png: vec![1],
                        width: 1,
                        height: 1
                    }
                )
            ],
            ClipboardDragMode::Auto,
            None
        )
        .is_err());
    }
    #[test]
    fn mixed_records_export_text_and_original_image_as_files() {
        let source = vec![137, 80, 78, 71, 1, 2, 3];
        let result = assemble(
            vec![
                record(1, ClipboardPayload::Text("文本".into())),
                record(
                    2,
                    ClipboardPayload::Image {
                        png: source.clone(),
                        width: 9000,
                        height: 9000,
                    },
                ),
            ],
            ClipboardDragMode::Auto,
            None,
        )
        .unwrap();
        assert_eq!(result.files.len(), 2);
        assert_eq!(result.exports[0].extension, "txt");
        assert_eq!(result.exports[1].bytes, source);
        assert!(result.text.is_none());
    }
    #[test]
    fn selected_text_is_bound_to_a_live_record_and_remains_plain() {
        let records = vec![record(1, ClipboardPayload::Text("hello\nworld".into()))];
        let selected = assemble(
            records.clone(),
            ClipboardDragMode::Auto,
            Some(ClipboardDragSelection {
                id: 1,
                text: "hello world".into(),
            }),
        )
        .unwrap();
        assert_eq!(selected.text.as_deref(), Some("hello world"));
        assert!(selected.html.is_none());
        assert!(assemble(
            records.clone(),
            ClipboardDragMode::Auto,
            Some(ClipboardDragSelection {
                id: 2,
                text: "hello".into()
            })
        )
        .is_err());
        assert!(assemble(
            records,
            ClipboardDragMode::Auto,
            Some(ClipboardDragSelection {
                id: 1,
                text: String::new()
            })
        )
        .is_err());
    }
    #[test]
    fn files_and_folders_are_checked_and_duplicates_are_removed() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("中文 #%.txt");
        std::fs::write(&file, "data").unwrap();
        let payload = ClipboardPayload::Files(vec![
            file.to_string_lossy().into(),
            root.path().to_string_lossy().into(),
            file.to_string_lossy().into(),
        ]);
        let result = assemble(
            vec![record(1, payload.clone())],
            ClipboardDragMode::Auto,
            None,
        )
        .unwrap();
        assert_eq!(result.files.len(), 2);
        std::fs::remove_file(file).unwrap();
        assert!(assemble(vec![record(1, payload)], ClipboardDragMode::Auto, None).is_err());
    }
}
