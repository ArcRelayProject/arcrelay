//! Native drag sources. Call `start` on the window's UI thread; completion is
//! delivered exactly once after a successfully started session has ended.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(target_os = "macos")]
#[path = "native_macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "native_windows.rs"]
mod platform;
#[cfg(target_os = "linux")]
#[path = "native_linux.rs"]
mod platform;

// The Windows wire encoders have no OS dependency, so their UTF-16/CF_HTML
// contracts are exercised by the native test suite on every platform.
#[cfg(all(test, not(target_os = "windows")))]
#[path = "native_windows.rs"]
mod windows_encoding_tests;

pub(super) struct NativeDragPayload {
    pub files: Vec<PathBuf>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub preview_png: Vec<u8>,
    pub cancelled: Arc<AtomicBool>,
    // Set immediately before handing the payload to the OS. The export lease
    // retains offered files for targets that read them after the drag ends.
    pub offered: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeDragOutcome {
    Dropped,
    Cancelled,
}

pub(super) type Completion = Box<dyn FnOnce(NativeDragOutcome) + Send + 'static>;

fn validate_payload(payload: &NativeDragPayload) -> Result<(), String> {
    if payload.files.is_empty() && payload.text.is_none() && payload.html.is_none() {
        return Err("clipboard drag has no content".into());
    }
    if !payload.files.is_empty() && (payload.text.is_some() || payload.html.is_some()) {
        return Err("file and text drag representations must be separate".into());
    }
    if payload.files.iter().any(|path| !path.is_absolute()) {
        return Err("clipboard drag file paths must be absolute".into());
    }
    if payload.preview_png.is_empty() {
        return Err("clipboard drag preview is missing".into());
    }
    Ok(())
}

pub(super) fn start(
    window: &tauri::WebviewWindow,
    payload: NativeDragPayload,
    completion: Completion,
) -> Result<(), String> {
    validate_payload(&payload)?;
    if payload.cancelled.load(Ordering::Acquire) {
        completion(NativeDragOutcome::Cancelled);
        return Ok(());
    }
    platform::start(window, payload, completion)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_payload() -> NativeDragPayload {
        NativeDragPayload {
            files: vec![],
            text: Some("你好\nclipboard".into()),
            html: Some("<p>你好</p>".into()),
            preview_png: vec![1],
            cancelled: Arc::new(AtomicBool::new(false)),
            offered: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn rich_text_is_one_drag_item_but_files_cannot_be_mixed_into_it() {
        let mut payload = text_payload();
        assert!(validate_payload(&payload).is_ok());
        payload.files.push(std::env::temp_dir().join("test.png"));
        assert!(validate_payload(&payload).is_err());
    }

    #[test]
    fn rejects_empty_payload_and_relative_export_paths() {
        let mut payload = text_payload();
        payload.text = None;
        payload.html = None;
        assert!(validate_payload(&payload).is_err());
        payload.files.push(PathBuf::from("relative.png"));
        assert!(validate_payload(&payload).is_err());
        payload.files = vec![std::env::temp_dir().join("absolute.png")];
        assert!(validate_payload(&payload).is_ok());
    }
}
